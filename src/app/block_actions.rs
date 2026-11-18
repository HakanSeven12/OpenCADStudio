use super::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::scene::block_actions::{self as actions, Kind};
use document::{EntityType, Handle};
use iced::Task;

mod preview;

// These grips start an ordinary point-input command, keeping snapping, typed
// coordinates and Escape on the command driver's existing path.
pub(super) const TEST_GRIP_START: usize = crate::scene::pick::grip::CUSTOM_GRIP_ID_START;
pub(super) fn is_test_grip(id: usize) -> bool {
    id >= TEST_GRIP_START && crate::scene::block_grips::runtime::decode(id).is_some()
}
pub(in crate::app) fn lookup_action_arg(args: &str) -> bool {
    matches!(args.trim().to_ascii_uppercase().as_str(), "LOOKUP" | "L")
}
impl OpenCADStudio {
    pub(in crate::app) fn action_association_error(&self) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            return Some("BASSOCIATE is only available in the Block Editor.");
        };
        let orphaned = actions::association::orphaned(&tab.scene.document, session.br_handle);
        if orphaned.is_empty() {
            return Some("This definition has no unassociated actions.");
        }
        let selected = tab
            .selected_authoring
            .filter(|selected| orphaned.contains(selected));
        let compatible = actions::actions(&tab.scene.document, session.br_handle)
            .into_iter()
            .filter(|(handle, _, _, _)| {
                orphaned.contains(handle) && selected.is_none_or(|selected| selected == *handle)
            })
            .any(|(_, _, kind, _)| {
                !actions::parameters(&tab.scene.document, session.br_handle, kind).is_empty()
            });
        if !compatible {
            return Some("Add a compatible parameter before associating an action.");
        }
        None
    }

    pub(in crate::app) fn start_block_action_association(&mut self) -> Task<Message> {
        if let Some(error) = self.action_association_error() {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let command = crate::modules::block_editor::associate::AssociateCommand::new(
            tab.id,
            tab.active_block_edit_session().unwrap().br_handle,
            &tab.scene.document,
            tab.selected_authoring,
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(in crate::app) fn reset_selected_block_references(&mut self, apply: bool) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .map(|(handle, _)| *handle)
            .collect::<Vec<_>>();
        if handles.is_empty() && !apply {
            let command = crate::modules::block_editor::reset::ResetCommand::new();
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Task::none();
        }
        let mut staged = self.tabs[i].scene.document.clone();
        let mut changed = 0;
        for handle in handles {
            let Some(definition) = staged.dynamic_definition_for_insert(handle) else {
                continue;
            };
            if self.tabs[i].scene.is_layer_locked(handle) {
                self.command_line.push_error(
                    crate::t!("RESETBLOCK: a selected reference is on a locked layer.").as_ref(),
                );
                return Task::none();
            }
            let result = if let Some(test) = &self.tabs[i].block_test {
                if handle != test.insert || definition != test.definition
                    || staged.entities().any(|entity| matches!(entity, EntityType::Insert(other) if other.common.handle != handle && staged.dynamic_definition_for_insert(other.common.handle) == Some(definition))) {
                    self.command_line.push_error(crate::t!("Reset the single test reference or reopen Test Block.").as_ref());
                    return Task::none();
                }
                actions::instance::reset_test(&staged, &test.baseline, definition, handle)
            } else {
                actions::instance::reset(&staged, handle)
            };
            match result {
                Ok(Some(next)) => {
                    staged = next;
                    changed += 1;
                }
                Ok(None) => {}
                Err(error) => {
                    self.command_line.push_error(crate::t!(error).as_ref());
                    return Task::none();
                }
            }
        }
        if changed != 0 {
            self.push_undo_snapshot(
                i,
                super::tab_document::HistoryLabel::Message("Reset block references"),
            );
            self.apply_action_document_and_bump(i, staged);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_properties();
        }
        self.command_line
            .push_output(crate::tf!("Reset {} dynamic block references.", changed).as_ref());
        Task::none()
    }

    pub(in crate::app) fn action_error(&self, kind: Option<Kind>) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(s) = tab.active_block_edit_session() else {
            return Some("Open a block in the Block Editor before adding actions.");
        };
        if let Some(e) =
            crate::scene::block_point_parameter::placement_error(&tab.scene.document, s.br_handle)
        {
            return Some(e);
        }
        let has_candidate = match kind {
            Some(kind) => !actions::parameters(&tab.scene.document, s.br_handle, kind).is_empty(),
            // The type prompt also offers Lookup, whose parameter is not a
            // geometry action target.
            None => {
                !actions::all_parameters(&tab.scene.document, s.br_handle).is_empty()
                    || !crate::scene::block_lookup::parameters(&tab.scene.document, s.br_handle)
                        .is_empty()
            }
        };
        if !has_candidate {
            return Some("Place a compatible parameter before adding this action.");
        }
        None
    }
    pub(in crate::app) fn action_set_error(&self) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            return Some("BACTIONSET is only available in the Block Editor.");
        };
        if let Some(e) = crate::scene::block_point_parameter::placement_error(
            &tab.scene.document,
            session.br_handle,
        ) {
            return Some(e);
        }
        actions::actions(&tab.scene.document, session.br_handle)
            .is_empty()
            .then_some("This block has no supported actions to edit.")
    }
    pub(in crate::app) fn start_block_action_set(&mut self) -> Task<Message> {
        if let Some(e) = self.action_set_error() {
            self.command_line.push_error(crate::t!(e).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let command = crate::modules::block_editor::action_set::ActionSetCommand::new(
            tab.id,
            tab.active_block_edit_session().unwrap().br_handle,
            &tab.scene.document,
            tab.selected_authoring,
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(super) fn apply_action_document_diff(
        &mut self,
        i: usize,
        mut staged: document::Document,
    ) -> (
        Vec<(document::Handle, crate::scene::ChangeKind)>,
        Vec<String>,
    ) {
        use crate::scene::ChangeKind;
        let doc = &mut self.tabs[i].scene.document;
        // Drawable entity iteration omits BLOCK and ENDBLK. Their records
        // still carry evaluated origins and delimit the published body.
        let markers = doc
            .block_records
            .iter()
            .chain(staged.block_records.iter())
            .flat_map(|record| [record.block_entity_handle, record.block_end_handle])
            .filter(|handle| !handle.is_null())
            .collect::<rustc_hash::FxHashSet<_>>();
        let mut seen_handles = rustc_hash::FxHashSet::default();
        let mut delta_changes = Vec::new();
        let changes = staged
            .entities()
            .chain(
                markers
                    .iter()
                    .filter_map(|&handle| staged.get_entity(handle)),
            )
            .filter_map(|e| {
                let handle = e.common().handle;
                if !seen_handles.insert(handle) {
                    return None;
                }
                let next_arc = staged.get_entity_arc(handle)?;
                match doc.get_entity_arc(handle) {
                    Some(prev_arc) if std::sync::Arc::ptr_eq(&prev_arc, &next_arc) => None,
                    Some(prev_arc) if *prev_arc == *next_arc => None,
                    Some(_) => {
                        delta_changes.push((handle, ChangeKind::Modified));
                        Some(e.clone())
                    }
                    None => {
                        delta_changes.push((handle, ChangeKind::Added));
                        Some(e.clone())
                    }
                }
            })
            .collect::<Vec<_>>();
        seen_handles.clear();
        let removed = doc
            .entities()
            .chain(markers.iter().filter_map(|&handle| doc.get_entity(handle)))
            .map(|e| e.common().handle)
            .filter(|h| seen_handles.insert(*h) && staged.get_entity(*h).is_none())
            .collect::<Vec<_>>();
        for &handle in &removed {
            delta_changes.push((handle, ChangeKind::Removed));
        }
        let mut touched_blocks = Vec::new();
        for next in staged.block_records.iter() {
            if doc.block_records.get(&next.name) != Some(next) {
                touched_blocks.push(next.name.clone());
            }
        }
        for prev in doc.block_records.iter() {
            if staged.block_records.get(&prev.name).is_none() {
                touched_blocks.push(prev.name.clone());
            }
        }
        doc.swap_structure(staged.snapshot_structure());
        for handle in removed {
            doc.remove_entity_arc(handle);
        }
        for entity in changes {
            let handle = entity.common().handle;
            if let Some(current) = doc.get_entity_mut(handle) {
                *current = entity;
            } else {
                doc.restore_entity_arc(std::sync::Arc::new(entity));
            }
        }
        (delta_changes, touched_blocks)
    }

    #[cfg(test)]
    pub(super) fn apply_action_document(&mut self, i: usize, staged: document::Document) {
        let _ = self.apply_action_document_diff(i, staged);
    }

    pub(super) fn apply_action_document_and_bump(
        &mut self,
        i: usize,
        staged: document::Document,
    ) {
        let (delta_changes, touched_blocks) = self.apply_action_document_diff(i, staged);
        let scene = &mut self.tabs[i].scene;
        for &(handle, _) in &delta_changes {
            scene.reseed_derived_caches(handle);
        }
        if delta_changes.is_empty() && touched_blocks.is_empty() {
            if let Some(edited) = scene.block_edit_block {
                scene.bump_block_definition(edited);
            }
            return;
        }
        scene.bump_entities_with_block_deps(&delta_changes, &[], &touched_blocks, false);
    }
    /// Lookup has no geometry action of its own: choose the parameter whose
    /// property table the new action edits.
    pub(in crate::app) fn start_block_lookup_action(
        &mut self,
        document: u64,
        definition: Handle,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].id != document
            || self.tabs[i]
                .active_block_edit_session()
                .map(|s| s.br_handle)
                != Some(definition)
        {
            self.command_line.push_error(
                crate::t!("The action's Block Editor session is no longer active.").as_ref(),
            );
            return Task::none();
        }
        if let Some(error) = self.lookup_table_error() {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        let candidates =
            crate::scene::block_lookup::parameters(&self.tabs[i].scene.document, definition)
                .into_iter()
                .map(
                    |(_, parameter)| crate::modules::block_editor::lookup_action::Candidate {
                        name: parameter.lookup_name.clone(),
                        point: actions::v(parameter.parameter.definition_point),
                    },
                )
                .collect();
        let command = crate::modules::block_editor::lookup_action::LookupActionCommand::new(
            document, definition, candidates,
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(in crate::app) fn start_block_action(&mut self, args: &str) -> Task<Message> {
        if lookup_action_arg(args) {
            let i = self.active_tab;
            let Some(definition) = self.tabs[i]
                .active_block_edit_session()
                .map(|session| session.br_handle)
            else {
                self.command_line.push_error(
                    crate::t!("Open a block in the Block Editor before adding actions.").as_ref(),
                );
                return Task::none();
            };
            return self.start_block_lookup_action(self.tabs[i].id, definition);
        }
        let kind = if args.is_empty() {
            None
        } else {
            Kind::parse(args)
        };
        if !args.is_empty() && kind.is_none() {
            self.command_line.push_error(
                crate::t!("Supported actions: Move, Scale, Stretch, Polar Stretch, Rotate, Flip, Array, Lookup.")
                    .as_ref(),
            );
            return Task::none();
        }
        if let Some(e) = self.action_error(kind) {
            self.command_line.push_error(crate::t!(e).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let definition = tab.active_block_edit_session().unwrap().br_handle;
        let command = crate::modules::block_editor::action::ActionCommand::new(
            tab.id,
            definition,
            kind,
            &tab.scene.document,
        );
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(in crate::app) fn start_block_action_for_parameter(
        &mut self,
        args: &str,
        parameter: document::Handle,
    ) -> Task<Message> {
        self.start_block_action_for_parameter_at(args, parameter, None)
    }
    fn start_block_action_for_parameter_at(
        &mut self,
        args: &str,
        parameter: document::Handle,
        xy_key_point: Option<actions::XyKeyPoint>,
    ) -> Task<Message> {
        let Some(kind) = Kind::parse(args) else {
            self.command_line.push_error(
                crate::t!("The Parameter Set's action type is not supported.").as_ref(),
            );
            return Task::none();
        };
        if let Some(error) = self.action_error(Some(kind)) {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let definition = tab.active_block_edit_session().unwrap().br_handle;
        let Some(command) = crate::modules::block_editor::action::ActionCommand::for_parameter(
            tab.id,
            definition,
            kind,
            &tab.scene.document,
            parameter,
            xy_key_point,
        ) else {
            self.command_line.push_error(
                crate::t!("The placed parameter is incompatible with this action.").as_ref(),
            );
            return Task::none();
        };
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
        Task::none()
    }
    pub(in crate::app) fn start_xy_box_set_action(
        &mut self,
        parameter: document::Handle,
        kind: Kind,
        key_point: u8,
    ) -> Task<Message> {
        let key_point = match key_point {
            0 => actions::XyKeyPoint::Base,
            1 => actions::XyKeyPoint::X,
            2 => actions::XyKeyPoint::Y,
            3 => actions::XyKeyPoint::End,
            _ => return Task::none(),
        };
        self.start_block_action_for_parameter_at(kind.name(), parameter, Some(key_point))
    }
    pub(in crate::app) fn start_parameter_pair_action(
        &mut self,
        parameter: document::Handle,
        kind: Kind,
        side: u8,
    ) -> Task<Message> {
        let key_point = match side {
            0 => actions::XyKeyPoint::Base,
            1 => actions::XyKeyPoint::End,
            _ => return Task::none(),
        };
        self.start_block_action_for_parameter_at(kind.name(), parameter, Some(key_point))
    }
    pub(in crate::app) fn handle_block_action_result(
        &mut self,
        result: crate::command::CmdResult,
    ) -> Task<Message> {
        let i = self.active_tab;
        match result {
            crate::command::CmdResult::AssociateBlockAction {
                document,
                definition,
                action,
                expected,
                parameter,
                key_point,
            } => {
                let tab = &self.tabs[i];
                if tab.id != document
                    || tab
                        .active_block_edit_session()
                        .map(|session| session.br_handle)
                        != Some(definition)
                    || tab.scene.document.objects.get(&action) != Some(expected.as_ref())
                {
                    self.command_line.push_error(
                        crate::t!(
                            "The action or its Block Editor session changed; restart BASSOCIATE."
                        )
                        .as_ref(),
                    );
                    return Task::none();
                }
                let mut staged = tab.scene.document.clone();
                let result = actions::association::associate(
                    &mut staged,
                    definition,
                    action,
                    parameter,
                    key_point,
                )
                .and_then(|changed| {
                    if changed && actions::association::orphaned(&staged, definition).is_empty() {
                        staged = actions::instance::refresh_after_edit(
                            &staged,
                            &tab.scene.document,
                            definition,
                        )?;
                    }
                    Ok(changed)
                });
                match result {
                    Ok(changed) => {
                        if changed {
                            self.push_undo_snapshot(
                                i,
                                super::tab_document::HistoryLabel::Message(
                                    "Associate block action",
                                ),
                            );
                            self.apply_action_document_and_bump(i, staged);
                            self.tabs[i].dirty = true;
                            self.finish_pending_history(i);
                        }
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.tabs[i].selected_authoring = Some(action);
                        self.refresh_properties();
                        self.command_line
                            .push_output(crate::t!("Action associated with parameter.").as_ref());
                    }
                    Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
                }
            }
            crate::command::CmdResult::EditBlockActionSet {
                document,
                definition,
                action,
                expected,
                point_choices,
                targets,
                frame,
                rotate_only,
            } => {
                let tab = &self.tabs[i];
                if tab.id != document
                    || tab.active_block_edit_session().map(|s| s.br_handle) != Some(definition)
                    || tab.scene.document.objects.get(&action) != Some(expected.as_ref())
                    || point_choices
                        .as_ref()
                        .is_some_and(|(target_kind, source, expected)| {
                            let current = actions::all_parameters(&tab.scene.document, definition)
                                .into_iter()
                                .filter(|p| p.is_action_target(*target_kind, *source))
                                .map(|p| (p.handle, p.id, p.name))
                                .collect::<Vec<_>>();
                            current != *expected
                        })
                {
                    self.command_line.push_error(
                        crate::t!(
                            "The action or its Block Editor session changed; restart BACTIONSET."
                        )
                        .as_ref(),
                    );
                    return Task::none();
                }
                let mut staged = tab.scene.document.clone();
                let result = actions::retarget_frame_points(
                    &mut staged,
                    definition,
                    action,
                    &targets,
                    frame.as_deref(),
                )
                .and_then(|changed| {
                    let rotation_changed = match rotate_only.as_deref() {
                        Some(targets) => actions::options::set_rotate_only(
                            &mut staged,
                            definition,
                            action,
                            targets,
                        )?,
                        None => false,
                    };
                    let changed = changed || rotation_changed;
                    if changed {
                        staged = actions::instance::refresh_after_edit(
                            &staged,
                            &tab.scene.document,
                            definition,
                        )?;
                    }
                    Ok(changed)
                });
                match result {
                    Ok(changed) => {
                        if changed {
                            self.push_undo_snapshot(
                                i,
                                super::tab_document::HistoryLabel::Message("Action selection set"),
                            );
                            self.apply_action_document_and_bump(i, staged);
                            self.tabs[i].dirty = true;
                            self.finish_pending_history(i);
                        }
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.refresh_properties();
                        self.command_line
                            .push_output(crate::t!("Action selection updated.").as_ref());
                    }
                    Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
                }
            }
            crate::command::CmdResult::PlaceBlockAction {
                document,
                definition,
                kind,
                parameter,
                xy_key_point,
                point_choices,
                targets,
                location,
                frame,
                array_offsets,
                displacement_overrides,
                rotate_only,
            } => {
                if self.tabs[i].id != document
                    || self.tabs[i]
                        .active_block_edit_session()
                        .map(|s| s.br_handle)
                        != Some(definition)
                {
                    self.command_line.push_error(
                        crate::t!("The action's Block Editor session is no longer active.")
                            .as_ref(),
                    );
                    return Task::none();
                }
                if point_choices
                    .as_ref()
                    .is_some_and(|(target_kind, source, expected)| {
                        let current =
                            actions::all_parameters(&self.tabs[i].scene.document, definition)
                                .into_iter()
                                .filter(|p| p.is_action_target(*target_kind, *source))
                                .map(|p| (p.handle, p.id, p.name))
                                .collect::<Vec<_>>();
                        current != *expected
                            || !actions::all_parameters(&self.tabs[i].scene.document, definition)
                                .iter()
                                .any(|p| p.handle == parameter && p.id == *source)
                    })
                {
                    self.command_line
                        .push_error(crate::tr!("block-action-targets", "create-stale").as_str());
                    return Task::none();
                }
                let mut staged = self.tabs[i].scene.document.clone();
                match actions::create_with_frame_points(
                    &mut staged,
                    definition,
                    kind,
                    parameter,
                    xy_key_point,
                    &targets,
                    location,
                    frame.as_deref(),
                    array_offsets,
                )
                .and_then(|handle| {
                    if let Some((multiplier, angle)) = displacement_overrides {
                        actions::options::set(&mut staged, definition, handle, multiplier, angle)?;
                    }
                    if kind == Kind::PolarStretch {
                        actions::options::set_rotate_only(
                            &mut staged,
                            definition,
                            handle,
                            &rotate_only,
                        )?;
                    } else if !rotate_only.is_empty() {
                        return Err("Rotate-only objects require a Polar Stretch action.");
                    }
                    if matches!(kind, Kind::Move | Kind::Flip)
                        && targets.iter().any(|h| staged.get_entity(*h).is_none())
                    {
                        staged = actions::instance::refresh_after_edit(
                            &staged,
                            &self.tabs[i].scene.document,
                            definition,
                        )?;
                    }
                    Ok(handle)
                }) {
                    Ok(_) => {
                        self.push_undo_snapshot(
                            i,
                            super::tab_document::HistoryLabel::formatted(
                                format!("{} action", kind.name()),
                                "action",
                                super::tab_document::HistoryLabel::Message(kind.name()),
                            ),
                        );
                        self.apply_action_document_and_bump(i, staged);
                        self.tabs[i].dirty = true;
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.finish_pending_history(i);
                        self.refresh_properties();
                        self.command_line.push_output(crate::tf!("{} action attached. Select the reference and use its parameter grip, or open Test Block.",crate::t!(kind.name())).as_ref());
                        if let Some((set_parameter, expected_kind, next_key_point)) =
                            self.tabs[i].pending_xy_box_set
                        {
                            let completed_key_point = match xy_key_point {
                                Some(actions::XyKeyPoint::Base) => 0,
                                Some(actions::XyKeyPoint::X) => 1,
                                Some(actions::XyKeyPoint::Y) => 2,
                                Some(actions::XyKeyPoint::End) => 3,
                                None => 3,
                            };
                            if set_parameter == parameter
                                && kind == expected_kind
                                && usize::from(next_key_point) == completed_key_point
                                && next_key_point < 3
                            {
                                let next = next_key_point + 1;
                                self.tabs[i].pending_xy_box_set = Some((parameter, kind, next));
                                return self.start_xy_box_set_action(parameter, kind, next);
                            }
                            self.tabs[i].pending_xy_box_set = None;
                            self.tabs[i].pending_set_snapshot = None;
                        }
                        if let Some((set_parameter, expected_kind, expected_side)) =
                            self.tabs[i].pending_action_pair
                        {
                            let completed_side = match xy_key_point {
                                Some(actions::XyKeyPoint::Base) => Some(0),
                                Some(actions::XyKeyPoint::End) | None => Some(1),
                                _ => None,
                            };
                            if set_parameter == parameter
                                && kind == expected_kind
                                && completed_side == Some(expected_side)
                                && expected_side == 0
                            {
                                self.tabs[i].pending_action_pair = Some((parameter, kind, 1));
                                return self.start_parameter_pair_action(parameter, kind, 1);
                            }
                            self.tabs[i].pending_action_pair = None;
                            self.tabs[i].pending_set_snapshot = None;
                        }
                    }
                    Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
                }
            }
            crate::command::CmdResult::EvaluateBlockParameter {
                grip,
                insert,
                document,
                definition,
                parameter,
                value,
            } => {
                if self.tabs[i].id != document {
                    return Task::none();
                }
                let source = self.tabs[i]
                    .block_test
                    .as_ref()
                    .filter(|test| insert.is_none() && test.definition == definition)
                    .map(|test| test.baseline.as_ref())
                    .unwrap_or(&self.tabs[i].scene.document);
                if grip.is_some_and(|id| {
                    !crate::scene::block_grips::runtime::action_grip_available(
                        source, definition, parameter, id,
                    )
                }) {
                    return Task::none();
                }
                if let Some(insert) = insert {
                    return self.evaluate_reference_parameter(insert, definition, parameter, value);
                }
                let Some(test) = self.tabs[i].block_test.as_ref() else {
                    return Task::none();
                };
                if test.definition != definition
                    || !actions::has_action(&test.baseline, definition, parameter)
                {
                    return Task::none();
                }
                let Some(EntityType::Insert(insert)) =
                    self.tabs[i].scene.document.get_entity(test.insert)
                else {
                    return Task::none();
                };
                if self.tabs[i].scene.is_layer_locked(test.insert) {
                    self.command_line
                        .push_error(crate::t!("The test reference is on a locked layer.").as_ref());
                    return Task::none();
                }
                let doc = &self.tabs[i].scene.document;
                if doc.entities().any(|e|matches!(e,EntityType::Insert(other) if other.common.handle!=test.insert && (other.block_name==insert.block_name||doc.dynamic_definition_for_insert(other.common.handle)==Some(definition)))) {
                    self.command_line.push_error(crate::t!("Test actions with one reference in Test Block. Remove the extra references or reopen Test Block.").as_ref());return Task::none();
                }
                let Some(inverse) =
                    crate::scene::inverse_affine(&crate::scene::render_graph::insert_transform(
                        &self.tabs[i].scene.document,
                        insert,
                    ))
                else {
                    self.command_line.push_error(
                        crate::t!("The test reference has a singular transform.").as_ref(),
                    );
                    return Task::none();
                };
                let local =
                    value.map_points(|point| actions::v(inverse.apply(actions::native(point))));
                match actions::instance::evaluate_test_value(
                    &self.tabs[i].scene.document,
                    &test.baseline,
                    definition,
                    test.insert,
                    parameter,
                    local,
                ) {
                    Ok(staged) => {
                        if let Some(staged) = staged {
                            self.push_undo_snapshot(
                                i,
                                super::tab_document::HistoryLabel::Message("Test block parameter"),
                            );
                            self.apply_action_document_and_bump(i, staged);
                            self.tabs[i].dirty = true;
                            self.finish_pending_history(i);
                        }
                        self.tabs[i].active_cmd = None;
                        self.tabs[i].scene.clear_preview_wire();
                        self.refresh_properties();
                    }
                    Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
                }
            }
            _ => unreachable!(),
        }
        Task::none()
    }
    pub(super) fn custom_block_grip_at_cursor(
        &self,
        i: usize,
        point: iced::Point,
        width: f32,
        height: f32,
    ) -> bool {
        use crate::scene::pick::grip::{
            find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, is_custom_block_marker,
        };
        let tab = &self.tabs[i];
        if !tab.selected_grips.iter().any(is_custom_block_marker) {
            return false;
        }
        let frame = tab.scene.viewport_edit_frame((width, height));
        let rect = frame
            .as_ref()
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| tab.scene.active_model_tile_bounds(width, height));
        let bounds = iced::Rectangle::with_size(iced::Size::new(rect.width, rect.height));
        let local = iced::Point::new(point.x - rect.x, point.y - rect.y);
        let hit = if let Some((camera, _)) = frame {
            find_hit_grip_rte(
                local,
                &tab.selected_grips,
                camera.view_proj_rte(bounds),
                camera.eye(),
                bounds,
            )
        } else if tab.scene.current_layout != "Model" {
            let camera = tab.scene.camera.borrow();
            let half_h = camera.ortho_size();
            let half_w = half_h * rect.width / rect.height.max(1.);
            find_hit_grip_paper(
                local,
                &tab.selected_grips,
                camera.target.x as f32,
                camera.target.y as f32,
                half_w,
                half_h,
                bounds,
            )
        } else {
            find_hit_grip(
                local,
                &tab.selected_grips,
                &tab.scene.camera.borrow(),
                bounds,
            )
        };
        hit.is_some_and(|(index, _, _, _)| {
            is_custom_block_marker(&tab.selected_grips[index])
                && tab.selected_grip_handles.get(index).is_some_and(|handle| {
                    matches!(
                        tab.scene.document.get_entity(*handle),
                        Some(EntityType::Insert(_))
                    )
                })
        })
    }

    pub(super) fn refresh_test_action_grips(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let Some(test) = tab.block_test.as_ref() else {
            self.refresh_reference_action_grips(wo);
            return;
        };
        if tab.selected_handle != Some(test.insert) {
            return;
        }
        let Some(EntityType::Insert(insert)) = tab.scene.document.get_entity(test.insert) else {
            return;
        };
        let transform = crate::scene::render_graph::insert_transform(&tab.scene.document, insert);
        let mut grips = vec![];
        let all = actions::actions(&test.baseline, test.definition);
        for p in actions::all_parameters(&test.baseline, test.definition) {
            if !all.iter().any(|(_, _, _, id)| *id == p.id) {
                continue;
            }
            let Ok(value) = actions::instance::test_value(&tab.scene.document, test.insert, &p)
            else {
                continue;
            };
            let Ok(p) =
                actions::instance::grip_parameter(&tab.scene.document, test.insert, &p, true)
            else {
                continue;
            };
            for grip in actions::instance::action_grips(
                &tab.scene.document,
                &test.baseline,
                test.definition,
                test.insert,
                &p,
                value,
                true,
            ) {
                grips.push(crate::scene::model::object::GripDef {
                    id: grip.id,
                    world: actions::v(transform.apply(actions::native(grip.point)))
                        - glam::DVec3::from_array(wo),
                    is_midpoint: false,
                    shape: parameter_grip_shape(&p),
                    dir: grip.direction.and_then(|direction| {
                        actions::v(transform.apply_rotation(actions::native(direction)))
                            .try_normalize()
                    }),
                    axis: None,
                });
            }
        }
        grips.extend(self.alignment_grip_markers(test.insert, wo));
        let handle = test.insert;
        let tab = &mut self.tabs[i];
        for grip in grips {
            tab.selected_grips.push(grip);
            tab.selected_grip_handles.push(handle);
        }
    }
    pub(super) fn start_test_action_grip(&mut self, handle: Handle, id: usize) -> bool {
        if !is_test_grip(id) {
            return false;
        }
        if self.start_alignment_grip(handle, id) {
            return true;
        }
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let Some(test) = tab.block_test.as_ref() else {
            return self.start_reference_action_grip(handle, id);
        };
        if handle != test.insert {
            return false;
        }
        if tab.scene.is_layer_locked(handle) {
            return true;
        }
        let Some((node, role)) = crate::scene::block_grips::runtime::decode(id) else {
            return false;
        };
        let Some(p) = actions::all_parameters(&test.baseline, test.definition)
            .into_iter()
            .find(|p| p.id == node)
        else {
            return false;
        };
        if !actions::has_action(&test.baseline, test.definition, p.handle) {
            return false;
        }
        let Ok(value) = actions::instance::test_value(&tab.scene.document, test.insert, &p) else {
            return false;
        };
        let Ok(p) = actions::instance::grip_parameter(&tab.scene.document, test.insert, &p, true)
        else {
            return false;
        };
        let Some(grip) = actions::instance::action_grips(
            &tab.scene.document,
            &test.baseline,
            test.definition,
            test.insert,
            &p,
            value,
            true,
        )
        .into_iter()
        .find(|grip| grip.role == role) else {
            return false;
        };
        if p.flip {
            let state = actions::instance::test_value(&tab.scene.document, test.insert, &p);
            let document = tab.id;
            let definition = test.definition;
            return self.toggle_action_grip(document, definition, p.handle, None, state);
        }
        let Some(EntityType::Insert(reference)) = tab.scene.document.get_entity(test.insert) else {
            return false;
        };
        let to_world = crate::scene::render_graph::insert_transform(&tab.scene.document, reference);
        let Some(to_local) = crate::scene::inverse_affine(&to_world) else {
            return false;
        };
        let Ok(frame) = actions::instance::test_frame(&tab.scene.document, test.insert, &p) else {
            return false;
        };
        let command = crate::modules::block_editor::action::TestParameterCommand {
            grip_input: Some(crate::scene::block_grips::runtime::Input {
                id,
                role,
                frame,
                offset: grip.offset,
                point: p.point,
                linear_axis: p.linear.then(|| (frame.end - frame.base).normalize()),
                limits: crate::scene::block_grips::runtime::Limits::read(
                    &test.baseline,
                    test.definition,
                    &p,
                ),
                to_world,
                to_local,
            }),
            display_offset: glam::DVec3::ZERO,
            insert: None,
            document: tab.id,
            definition: test.definition,
            parameter: p.handle,
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }
}

impl OpenCADStudio {
    /// Parameter grips of every selected dynamic block reference; the
    /// alignment markers only for a single selected reference.
    fn refresh_reference_action_grips(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        if self.tabs[i].active_block_edit_session().is_some() {
            return;
        }
        let handles = self.tabs[i].scene.selected_handles_in_order();
        let limit = self.grip_object_limit;
        if limit > 0 && handles.len() > limit as usize {
            return;
        }
        let single = handles.len() == 1;
        for handle in handles {
            self.push_reference_action_grips(handle, single, wo);
        }
    }
    fn push_reference_action_grips(&mut self, handle: Handle, alignment: bool, wo: [f64; 3]) {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let Some(def) = actions::instance::definition(&tab.scene.document, handle) else {
            return;
        };
        let Some(EntityType::Insert(reference)) = tab.scene.document.get_entity(handle) else {
            return;
        };
        let transform =
            crate::scene::render_graph::insert_transform(&tab.scene.document, reference);
        let all = actions::actions(&tab.scene.document, def);
        let mut grips = vec![];
        for p in actions::all_parameters(&tab.scene.document, def) {
            if !all.iter().any(|(_, _, _, id)| *id == p.id) {
                continue;
            }
            let Ok(value) = actions::instance::value(&tab.scene.document, handle, &p) else {
                continue;
            };
            let Ok(p) = actions::instance::grip_parameter(&tab.scene.document, handle, &p, false)
            else {
                continue;
            };
            for grip in actions::instance::action_grips(
                &tab.scene.document,
                &tab.scene.document,
                def,
                handle,
                &p,
                value,
                false,
            ) {
                grips.push(crate::scene::model::object::GripDef {
                    id: grip.id,
                    world: actions::v(transform.apply(actions::native(grip.point)))
                        - glam::DVec3::from_array(wo),
                    is_midpoint: false,
                    shape: parameter_grip_shape(&p),
                    dir: grip.direction.and_then(|direction| {
                        actions::v(transform.apply_rotation(actions::native(direction)))
                            .try_normalize()
                    }),
                    axis: None,
                });
            }
        }
        if alignment {
            grips.extend(self.alignment_grip_markers(handle, wo));
        }
        for grip in grips {
            self.tabs[i].selected_grips.push(grip);
            self.tabs[i].selected_grip_handles.push(handle);
        }
    }
    fn start_reference_action_grip(&mut self, handle: Handle, id: usize) -> bool {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_block_edit_session().is_some() || !tab.scene.selected.contains(&handle) {
            return false;
        }
        let Some(def) = actions::instance::definition(&tab.scene.document, handle) else {
            return false;
        };
        if tab.scene.is_layer_locked(handle) {
            return true;
        }
        let Some((node, role)) = crate::scene::block_grips::runtime::decode(id) else {
            return false;
        };
        let Some(p) = actions::all_parameters(&tab.scene.document, def)
            .into_iter()
            .find(|p| p.id == node)
        else {
            return false;
        };
        if !actions::has_action(&tab.scene.document, def, p.handle) {
            return false;
        }
        let Ok(value) = actions::instance::value(&tab.scene.document, handle, &p) else {
            return false;
        };
        let Ok(p) = actions::instance::grip_parameter(&tab.scene.document, handle, &p, false)
        else {
            return false;
        };
        let Some(grip) = actions::instance::action_grips(
            &tab.scene.document,
            &tab.scene.document,
            def,
            handle,
            &p,
            value,
            false,
        )
        .into_iter()
        .find(|grip| grip.role == role) else {
            return false;
        };
        if p.flip {
            let state = actions::instance::value(&tab.scene.document, handle, &p);
            let document = tab.id;
            return self.toggle_action_grip(document, def, p.handle, Some(handle), state);
        }
        let Some(EntityType::Insert(reference)) = tab.scene.document.get_entity(handle) else {
            return false;
        };
        let to_world = crate::scene::render_graph::insert_transform(&tab.scene.document, reference);
        let Some(to_local) = crate::scene::inverse_affine(&to_world) else {
            return false;
        };
        let Ok(frame) = actions::instance::frame(&tab.scene.document, handle, &p) else {
            return false;
        };
        let command = crate::modules::block_editor::action::TestParameterCommand {
            grip_input: Some(crate::scene::block_grips::runtime::Input {
                id,
                role,
                frame,
                offset: grip.offset,
                point: p.point,
                linear_axis: p.linear.then(|| (frame.end - frame.base).normalize()),
                limits: crate::scene::block_grips::runtime::Limits::read(
                    &tab.scene.document,
                    def,
                    &p,
                ),
                to_world,
                to_local,
            }),
            display_offset: glam::DVec3::ZERO,
            insert: Some(handle),
            document: tab.id,
            definition: def,
            parameter: p.handle,
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }
    fn evaluate_reference_parameter(
        &mut self,
        handle: Handle,
        definition: Handle,
        parameter: Handle,
        value: actions::Value,
    ) -> Task<Message> {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_block_edit_session().is_some()
            || tab.block_test.is_some()
            || actions::instance::definition(&tab.scene.document, handle) != Some(definition)
            || !actions::has_action(&tab.scene.document, definition, parameter)
        {
            return Task::none();
        }
        if tab.scene.is_layer_locked(handle) {
            return Task::none();
        }
        let Some(EntityType::Insert(reference)) = tab.scene.document.get_entity(handle) else {
            return Task::none();
        };
        let Some(inverse) = crate::scene::inverse_affine(
            &crate::scene::render_graph::insert_transform(&tab.scene.document, reference),
        ) else {
            return Task::none();
        };
        let local = value.map_points(|point| actions::v(inverse.apply(actions::native(point))));
        match actions::instance::evaluate_value(&tab.scene.document, handle, parameter, local) {
            Ok(staged) => {
                if let Some(staged) = staged {
                    self.push_undo_snapshot(
                        i,
                        super::tab_document::HistoryLabel::Message("Block reference parameter"),
                    );
                    self.apply_action_document_and_bump(i, staged);
                    self.tabs[i].dirty = true;
                    self.finish_pending_history(i);
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].scene.clear_preview_wire();
                self.refresh_properties();
            }
            Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
        }
        Task::none()
    }
}

impl OpenCADStudio {
    fn toggle_action_grip(
        &mut self,
        document: u64,
        definition: Handle,
        parameter: Handle,
        insert: Option<Handle>,
        state: Result<actions::Value, &'static str>,
    ) -> bool {
        match state {
            Ok(actions::Value::Flip(state)) => {
                let _ = self.handle_block_action_result(
                    crate::command::CmdResult::EvaluateBlockParameter {
                        grip: None,
                        document,
                        definition,
                        parameter,
                        insert,
                        value: actions::Value::Flip(!state),
                    },
                );
            }
            Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
            _ => return false,
        }
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }
}

#[doc(hidden)]
pub struct BlockBenchHarness(OpenCADStudio);

#[doc(hidden)]
pub struct BlockBenchClipboard {
    entities: Vec<EntityType>,
    deps: crate::app::ClipboardDeps,
}

impl BlockBenchHarness {
    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new();
        app.apply_config(crate::app::config::AppConfig::default());
        app.last_saved_config = Some(app.current_config());
        let _ = app.update(Message::TabNew);
        app
    }

    fn populate_background(scene: &mut crate::scene::Scene, bg_lines: usize, other_blocks: usize, entities_per_block: usize, prefix: &str) {
        use document::entities::{Insert, Line};
        use document::types::Vector3;

        for k in 0..bg_lines {
            let y = (k as f64) * 2.0;
            scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::new(100.0, y, 0.0),
                Vector3::new(200.0, y, 0.0),
            )));
        }
        for b in 0..other_blocks {
            let name = format!("{prefix}_{b}");
            let ents = (0..entities_per_block)
                .map(|j| {
                    EntityType::Line(Line::from_points(
                        Vector3::new(0.0, (j as f64) * 0.5, 0.0),
                        Vector3::new(5.0, (j as f64) * 0.5, 0.0),
                    ))
                })
                .collect();
            scene.define_block_raw(&name, Vector3::ZERO, ents);
            let mut ins = Insert::new(&name, Vector3::new((b as f64) * 10.0, -50.0, 0.0));
            ins.common.handle = Handle::NULL;
            scene.add_entity(EntityType::Insert(ins));
        }
    }

    fn setup_edit_block_app(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> (OpenCADStudio, Handle) {
        use document::entities::{Insert, Line};
        use document::types::Vector3;

        let mut app = Self::new_app();
        let i = app.active_tab;
        Self::populate_background(
            &mut app.tabs[i].scene,
            bg_lines,
            other_blocks,
            entities_per_block,
            "OtherBlock",
        );
        let edit_ents = (0..entities_per_block)
            .map(|j| {
                EntityType::Line(Line::from_points(
                    Vector3::new(0.0, (j as f64) * 0.5, 0.0),
                    Vector3::new(8.0, (j as f64) * 0.5, 0.0),
                ))
            })
            .collect();
        app.tabs[i]
            .scene
            .define_block_raw("EditBenchBlock", Vector3::ZERO, edit_ents);
        let edit_line = app.tabs[i]
            .scene
            .document
            .block_records
            .get("EditBenchBlock")
            .unwrap()
            .entity_handles[0];
        let mut ins_edit = Insert::new("EditBenchBlock", Vector3::new(10.0, 20.0, 0.0));
        ins_edit.common.handle = Handle::NULL;
        app.tabs[i].scene.add_entity(EntityType::Insert(ins_edit));
        let _ = app.tabs[i].scene.entity_wires();
        (app, edit_line)
    }

    pub fn setup_block_edit_undo(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> Self {
        let (mut app, edit_line) =
            Self::setup_edit_block_app(bg_lines, other_blocks, entities_per_block);
        let i = app.active_tab;

        app.push_undo_snapshot(
            i,
            super::tab_document::HistoryLabel::Message("Edit block geometry"),
        );
        if let Some(EntityType::Line(line)) = app.tabs[i].scene.document.get_entity_mut(edit_line)
        {
            line.end.x = 16.0;
        }
        app.tabs[i].scene.bump_geometry();
        app.finish_pending_history(i);
        let _ = app.tabs[i].scene.entity_wires();
        Self(app)
    }

    pub fn undo_redo_cycle(&mut self) -> usize {
        let i = self.0.active_tab;
        self.0.undo_active_tab();
        let a = self.0.tabs[i].scene.entity_wires().len();
        self.0.redo_active_tab();
        let b = self.0.tabs[i].scene.entity_wires().len();
        a.wrapping_add(b)
    }

    pub fn setup_visibility(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> (Self, Handle, Handle) {
        let (scene, _insert, def, param) = crate::scene::Scene::bench_setup_visibility_scene(
            bg_lines,
            other_blocks,
            entities_per_block,
        );
        let mut app = Self::new_app();
        let i = app.active_tab;
        app.tabs[i].scene = scene;
        let _ = app.tabs[i].scene.entity_wires();
        app.push_undo_snapshot(
            i,
            super::tab_document::HistoryLabel::Message("Visibility state"),
        );
        app.show_visibility_state(i, def, param, 1);
        app.finish_pending_history(i);
        let _ = app.tabs[i].scene.entity_wires();
        (Self(app), def, param)
    }

    pub fn show_visibility_state(
        &mut self,
        def_block: Handle,
        param: Handle,
        state_idx: usize,
    ) -> usize {
        let i = self.0.active_tab;
        self.0.show_visibility_state(i, def_block, param, state_idx);
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_dynamic_action(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> (Self, Handle, Handle, Handle) {
        use document::entities::{Insert, Line};
        use document::types::Vector3;

        let (mut scene, insert, stretch_param, _flip_param) =
            crate::scene::Scene::bench_setup_dynamic_block_scene(bg_lines, 0, entities_per_block);
        for b in 0..other_blocks {
            let name = format!("ActionOtherBlock_{b}");
            let ents = (0..entities_per_block)
                .map(|j| {
                    EntityType::Line(Line::from_points(
                        Vector3::new(0.0, (j as f64) * 0.5, 0.0),
                        Vector3::new(5.0, (j as f64) * 0.5, 0.0),
                    ))
                })
                .collect();
            scene.define_block_raw(&name, Vector3::ZERO, ents);
            let mut ins = Insert::new(&name, Vector3::new((b as f64) * 10.0, -50.0, 0.0));
            ins.common.handle = Handle::NULL;
            scene.add_entity(EntityType::Insert(ins));
        }
        let def = actions::instance::definition(&scene.document, insert).expect("dynamic def");
        let _ = scene.entity_wires();
        let mut app = Self::new_app();
        let i = app.active_tab;
        app.tabs[i].scene = scene;
        let _ = app.tabs[i].scene.entity_wires();
        (Self(app), insert, def, stretch_param)
    }

    pub fn evaluate_dynamic_action(
        &mut self,
        insert: Handle,
        definition: Handle,
        parameter: Handle,
        stretch_x: f64,
    ) -> usize {
        let _ = self.0.evaluate_reference_parameter(
            insert,
            definition,
            parameter,
            actions::Value::Point(glam::DVec3::new(stretch_x, 0.0, 0.0)),
        );
        let i = self.0.active_tab;
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_block_prop_edit(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> (Self, Handle) {
        let (app, edit_line) =
            Self::setup_edit_block_app(bg_lines, other_blocks, entities_per_block);
        (Self(app), edit_line)
    }

    pub fn prop_edit_in_block(&mut self, member: Handle, step: usize) -> usize {
        let i = self.0.active_tab;
        if let Some(EntityType::Line(line)) = self.0.tabs[i].scene.document.get_entity_mut(member) {
            line.end.x = 12.0 + (step % 5) as f64;
        }
        self.0.invalidate_property_targets_or_blocks(i, &[member]);
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_bedit_save(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> (Self, Handle) {
        let (mut scene, _insert, _stretch_param, _flip_param) =
            crate::scene::Scene::bench_setup_dynamic_block_scene(bg_lines, 0, entities_per_block);
        Self::populate_background(
            &mut scene,
            0,
            other_blocks,
            entities_per_block,
            "SaveOtherBlock",
        );
        let edit_line = scene
            .document
            .block_records
            .get("DynBenchBlock")
            .unwrap()
            .entity_handles[0];
        let mut app = Self::new_app();
        let i = app.active_tab;
        app.tabs[i].scene = scene;
        let _ = app.tabs[i].scene.entity_wires();
        (Self(app), edit_line)
    }

    pub fn bedit_save_step(&mut self, member: Handle, step: usize) -> usize {
        let _ = self.0.dispatch_command("BEDIT_OPEN:DynBenchBlock");
        let i = self.0.active_tab;
        assert!(self.0.tabs[i].active_block_edit.is_some());
        self.0.push_undo_snapshot(
            i,
            super::tab_document::HistoryLabel::Message("Edit block geometry"),
        );
        if let Some(EntityType::Line(line)) = self.0.tabs[i].scene.document.get_entity_mut(member) {
            line.end.x = 12.0 + (step % 5) as f64;
        }
        self.0.tabs[i]
            .scene
            .bump_entities(&[(member, crate::scene::ChangeKind::Modified)]);
        self.0.finish_pending_history(i);
        assert!(self.0.save_block_edit(i));
        self.0.close_block_edit(i, false);
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_find_replace(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> Self {
        use document::entities::{Insert, Text};
        use document::types::Vector3;

        let mut app = Self::new_app();
        let i = app.active_tab;
        Self::populate_background(
            &mut app.tabs[i].scene,
            bg_lines,
            other_blocks,
            entities_per_block,
            "FindOtherBlock",
        );

        let mut top_text = Text::new();
        top_text.value = "FIND_TOK_0".into();
        top_text.height = 2.5;
        app.tabs[i].scene.add_entity(EntityType::Text(top_text));

        let mut blk_text = Text::new();
        blk_text.value = "FIND_TOK_0".into();
        blk_text.height = 2.5;
        app.tabs[i].scene.define_block_raw(
            "FindBenchBlock",
            Vector3::ZERO,
            vec![EntityType::Text(blk_text)],
        );
        let mut ins = Insert::new("FindBenchBlock", Vector3::new(15.0, 25.0, 0.0));
        ins.common.handle = Handle::NULL;
        app.tabs[i].scene.add_entity(EntityType::Insert(ins));
        app.find_replace.options.blocks = true;
        let _ = app.tabs[i].scene.entity_wires();
        Self(app)
    }

    pub fn find_replace_cycle(&mut self, step: usize) -> usize {
        use crate::ui::window::find_replace::{FindAnswer, FindMsg};

        let (search, replace) = if step % 2 == 0 {
            ("FIND_TOK_0", "FIND_TOK_1")
        } else {
            ("FIND_TOK_1", "FIND_TOK_0")
        };
        self.0.find_replace.changed.clear();
        self.0.find_replace.search = search.to_string();
        self.0.find_replace.replacement = replace.to_string();
        let _ = self.0.find_replace_all();
        if self.0.find_replace.run.is_some() {
            let _ = self
                .0
                .on_find_message(FindMsg::Answer(FindAnswer::AllBlocks));
        }
        let i = self.0.active_tab;
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_battman(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> Self {
        use document::entities::{AttributeDefinition, AttributeEntity, Insert, Line};
        use document::types::Vector3;

        let mut app = Self::new_app();
        let i = app.active_tab;
        Self::populate_background(
            &mut app.tabs[i].scene,
            bg_lines,
            other_blocks,
            entities_per_block,
            "AttOtherBlock",
        );

        let att1 = AttributeDefinition::new("TAG_A".into(), "Prompt A".into(), "VAL_0".into());
        let att2 = AttributeDefinition::new("TAG_B".into(), "Prompt B".into(), "INIT".into());
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(5.0, 0.0, 0.0));
        line.common.handle = Handle::NULL;
        app.tabs[i].scene.define_block_raw(
            "AttBenchBlock",
            Vector3::ZERO,
            vec![
                EntityType::Line(line),
                EntityType::AttributeDefinition(att1.clone()),
                EntityType::AttributeDefinition(att2.clone()),
            ],
        );
        for k in 0..5 {
            let mut ins = Insert::new("AttBenchBlock", Vector3::new((k as f64) * 12.0, 40.0, 0.0));
            ins.common.handle = Handle::NULL;
            ins.attributes
                .push(AttributeEntity::from_definition(&att1, Some("VAL_0".into())));
            ins.attributes
                .push(AttributeEntity::from_definition(&att2, Some("INIT".into())));
            app.tabs[i].scene.add_entity(EntityType::Insert(ins));
        }
        let _ = app.tabs[i].scene.entity_wires();
        let _ = app.open_attman(i, &[]);
        Self(app)
    }

    pub fn battman_apply(&mut self, step: usize) -> usize {
        use crate::ui::window::attman_dialog::AttManMsg;

        let tag = if step % 2 == 0 { "TAG_X" } else { "TAG_A" };
        let _ = self.0.on_attman(AttManMsg::Tag(tag.to_string()));
        let _ = self.0.on_attman(AttManMsg::Sync);
        let i = self.0.active_tab;
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_bcycleorder(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> Self {
        use document::entities::{Insert, Line};
        use document::types::Vector3;

        let (mut scene, _insert, _stretch_param, _flip_param) =
            crate::scene::Scene::bench_setup_dynamic_block_scene(bg_lines, 0, entities_per_block);
        for b in 0..other_blocks {
            let name = format!("CycleOtherBlock_{b}");
            let ents = (0..entities_per_block)
                .map(|j| {
                    EntityType::Line(Line::from_points(
                        Vector3::new(0.0, (j as f64) * 0.5, 0.0),
                        Vector3::new(5.0, (j as f64) * 0.5, 0.0),
                    ))
                })
                .collect();
            scene.define_block_raw(&name, Vector3::ZERO, ents);
            let mut ins = Insert::new(&name, Vector3::new((b as f64) * 10.0, -50.0, 0.0));
            ins.common.handle = Handle::NULL;
            scene.add_entity(EntityType::Insert(ins));
        }
        let mut app = Self::new_app();
        let i = app.active_tab;
        app.tabs[i].scene = scene;
        let _ = app.tabs[i].scene.entity_wires();
        Self(app)
    }

    pub fn bcycleorder_apply(&mut self) -> usize {
        let _ = self.0.dispatch_command("BEDIT_OPEN:DynBenchBlock");
        let i = self.0.active_tab;
        assert!(self.0.tabs[i].active_block_edit.is_some());
        let _ = self.0.open_block_cycle_order();
        assert!(self
            .0
            .block_cycle_order
            .as_ref()
            .is_some_and(|s| s.rows.len() >= 2));
        let _ = self.0.update(Message::BlockCycleEnable(0, true));
        let _ = self.0.update(Message::BlockCycleEnable(1, true));
        let _ = self.0.update(Message::BlockCycleMove(0, false));
        let _ = self.0.update(Message::BlockCycleApply);
        self.0.close_block_edit(i, false);
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_background(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> Self {
        let mut app = Self::new_app();
        let i = app.active_tab;
        Self::populate_background(
            &mut app.tabs[i].scene,
            bg_lines,
            other_blocks,
            entities_per_block,
            "OtherBlock",
        );
        let _ = app.tabs[i].scene.entity_wires();
        Self(app)
    }

    pub fn setup_edit_block(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
    ) -> (Self, Handle) {
        let (app, member) =
            Self::setup_edit_block_app(bg_lines, other_blocks, entities_per_block);
        (Self(app), member)
    }

    pub fn define_block_owned_step(&mut self, step: usize, entities_per_block: usize) -> usize {
        use document::entities::Line;
        use document::types::Vector3;

        let i = self.0.active_tab;
        let ents = (0..entities_per_block)
            .map(|j| {
                EntityType::Line(Line::from_points(
                    Vector3::new(0.0, (j as f64) * 0.5, 0.0),
                    Vector3::new(6.0, (j as f64) * 0.5, 0.0),
                ))
            })
            .collect();
        let name = format!("OwnedBench_{step}");
        self.0.tabs[i]
            .scene
            .define_block_from_owned_entities(ents, &name, glam::DVec3::ZERO)
            .expect("define_block_from_owned_entities should succeed");
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn redefine_block_step(&mut self, step: usize, entities_per_block: usize) -> usize {
        use document::entities::Line;
        use document::types::Vector3;

        let i = self.0.active_tab;
        let delta = ((step + 1) as f64) * 0.1;
        let ents = (0..entities_per_block)
            .map(|j| {
                EntityType::Line(Line::from_points(
                    Vector3::new(0.0, (j as f64) * 0.5 + delta, 0.0),
                    Vector3::new(8.0, (j as f64) * 0.5 + delta, 0.0),
                ))
            })
            .collect();
        self.0.tabs[i]
            .scene
            .redefine_block_raw("EditBenchBlock", Vector3::ZERO, ents)
            .expect("redefine_block_raw should succeed");
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn rename_block_step(&mut self, step: usize) -> usize {
        let i = self.0.active_tab;
        let old = if step == 0 {
            "EditBenchBlock".to_string()
        } else {
            format!("EditBenchBlock_{step}")
        };
        let new = format!("EditBenchBlock_{}", step + 1);
        crate::scene::block_service::rename(&mut self.0.tabs[i].scene, &old, &new, &[])
            .expect("block_service::rename should succeed");
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_delete_blocks(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
        count: usize,
    ) -> Self {
        use document::entities::{Insert, Line};
        use document::types::Vector3;

        let mut app = Self::new_app();
        let i = app.active_tab;
        Self::populate_background(
            &mut app.tabs[i].scene,
            bg_lines,
            other_blocks,
            entities_per_block,
            "OtherBlock",
        );
        for d in 0..count {
            let name = format!("DelBench_{d}");
            let ents = (0..entities_per_block)
                .map(|j| {
                    EntityType::Line(Line::from_points(
                        Vector3::new(0.0, (j as f64) * 0.5, 0.0),
                        Vector3::new(4.0, (j as f64) * 0.5, 0.0),
                    ))
                })
                .collect();
            app.tabs[i]
                .scene
                .define_block_raw(&name, Vector3::ZERO, ents);
            let mut ins = Insert::new(&name, Vector3::new((d as f64) * 10.0, -90.0, 0.0));
            ins.common.handle = Handle::NULL;
            app.tabs[i].scene.add_entity(EntityType::Insert(ins));
        }
        let _ = app.tabs[i].scene.entity_wires();
        Self(app)
    }

    pub fn delete_block_step(&mut self, step: usize) -> usize {
        let i = self.0.active_tab;
        let name = format!("DelBench_{step}");
        crate::scene::block_service::delete(
            &mut self.0.tabs[i].scene,
            &name,
            crate::scene::block_service::DeleteOptions {
                force: true,
                open_edits: &[],
            },
        )
        .expect("block_service::delete should succeed");
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_laydel_in_block(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
        count: usize,
    ) -> Self {
        use document::entities::{Insert, Line};
        use document::tables::Layer;
        use document::types::Vector3;

        let mut app = Self::new_app();
        let i = app.active_tab;
        Self::populate_background(
            &mut app.tabs[i].scene,
            bg_lines,
            other_blocks,
            entities_per_block,
            "OtherBlock",
        );
        let mut ents = Vec::with_capacity(count);
        for k in 0..count {
            let layer_name = format!("DelLayer_{k}");
            let mut layer = Layer::new(&layer_name);
            let lh = app.tabs[i].scene.document.allocate_handle();
            layer.handle = lh;
            let _ = app.tabs[i].scene.document.layers.add(layer);

            let mut line = Line::from_points(
                Vector3::new(0.0, (k as f64) * 0.5, 0.0),
                Vector3::new(6.0, (k as f64) * 0.5, 0.0),
            );
            line.common.layer = layer_name;
            ents.push(EntityType::Line(line));
        }
        app.tabs[i]
            .scene
            .define_block_raw("LaydelBenchBlock", Vector3::ZERO, ents);
        let mut ins = Insert::new("LaydelBenchBlock", Vector3::new(0.0, -75.0, 0.0));
        ins.common.handle = Handle::NULL;
        app.tabs[i].scene.add_entity(EntityType::Insert(ins));
        let _ = app.tabs[i].scene.entity_wires();
        Self(app)
    }

    pub fn laydel_in_block_step(&mut self, step: usize) -> usize {
        let cmd = format!("LAYDEL DelLayer_{step}");
        let _ = self.0.dispatch_command(&cmd);
        let i = self.0.active_tab;
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn update_block_member_step(&mut self, member: Handle, step: usize) -> usize {
        let i = self.0.active_tab;
        let mut updated = self.0.tabs[i]
            .scene
            .document
            .get_entity(member)
            .expect("block member exists")
            .clone();
        if let EntityType::Line(ref mut l) = updated {
            l.end.y = ((step % 17) as f64) + 1.5;
        }
        self.0.tabs[i].scene.update_entity(updated);
        self.0.tabs[i].scene.entity_wires().len()
    }

    pub fn setup_paste_new_block(
        bg_lines: usize,
        other_blocks: usize,
        entities_per_block: usize,
        count: usize,
    ) -> (Self, Vec<BlockBenchClipboard>) {
        use document::entities::{Insert, Line};
        use document::types::Vector3;

        let mut app = Self::new_app();
        let target = app.active_tab;
        Self::populate_background(
            &mut app.tabs[target].scene,
            bg_lines,
            other_blocks,
            entities_per_block,
            "OtherBlock",
        );
        let _ = app.tabs[target].scene.entity_wires();

        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(99));
        let source = app.tabs.len() - 1;
        let mut snapshots = Vec::with_capacity(count);
        for k in 0..count {
            let name = format!("PasteBench_{k}");
            let ents = (0..entities_per_block)
                .map(|j| {
                    EntityType::Line(Line::from_points(
                        Vector3::new(0.0, (j as f64) * 0.5, 0.0),
                        Vector3::new(7.0, (j as f64) * 0.5, 0.0),
                    ))
                })
                .collect();
            app.tabs[source]
                .scene
                .define_block_raw(&name, Vector3::ZERO, ents);
            let mut ins = Insert::new(&name, Vector3::ZERO);
            ins.common.handle = Handle::NULL;
            let ins_h = app.tabs[source].scene.add_entity(EntityType::Insert(ins));
            let (entities, deps) = crate::app::command_driver::clipboard::copy_to_clipboard_kernel(
                &app.tabs[source].scene.document,
                &[ins_h],
            );
            snapshots.push(BlockBenchClipboard { entities, deps });
        }
        app.tabs.pop();
        app.active_tab = target;
        (Self(app), snapshots)
    }

    pub fn paste_new_block_step(&mut self, snap: &BlockBenchClipboard, step: usize) -> usize {
        let i = self.0.active_tab;
        self.0.clipboard = snap.entities.clone();
        self.0.clipboard_deps = snap.deps.clone();
        let _ = self.0.finalize_paste(
            i,
            Some(crate::command::EntityTransform::Translate(
                glam::DVec3::new((step as f64) * 15.0, 20.0, 0.0),
            )),
        );
        self.0.tabs[i].scene.entity_wires().len()
    }
}

/// Grip shape by parameter kind: flip = arrow, rotation = circle,
/// linear = triangle, point / XY / polar = square.
fn parameter_grip_shape(p: &actions::Parameter) -> crate::scene::model::object::GripShape {
    use crate::scene::model::object::GripShape;
    if p.flip {
        GripShape::Arrow
    } else if p.rotation {
        GripShape::Circle
    } else if p.linear {
        GripShape::Triangle
    } else {
        GripShape::Square
    }
}

#[cfg(test)]
mod item1_incremental_tests {
    use super::*;

    fn first_top_level_line(app: &OpenCADStudio) -> Handle {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|e| matches!(e, EntityType::Line(_)).then(|| e.common().handle))
            .expect("top-level line")
    }

    #[test]
    fn invalidate_property_targets_or_blocks_is_incremental_for_block_members() {
        let (mut harness, member) = BlockBenchHarness::setup_block_prop_edit(5, 2, 2);
        let i = harness.0.active_tab;
        let top_line = first_top_level_line(&harness.0);
        let keep_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr("OtherBlock_0")
            .unwrap();
        let edit_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr("EditBenchBlock")
            .unwrap();
        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));

        harness.prop_edit_in_block(member, 1);

        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));
        assert_eq!(
            keep_before,
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr("OtherBlock_0")
                .unwrap()
        );
        assert_ne!(
            edit_before,
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr("EditBenchBlock")
                .unwrap()
        );
    }

    #[test]
    fn bedit_save_preserves_unrelated_blocks_and_top_level_cache() {
        let (mut harness, member) = BlockBenchHarness::setup_bedit_save(5, 2, 2);
        let i = harness.0.active_tab;
        let top_line = first_top_level_line(&harness.0);
        let keep_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr("SaveOtherBlock_0")
            .unwrap();
        let insert_block = harness.0.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|e| match e {
                EntityType::Insert(ins) if !ins.block_name.starts_with("SaveOtherBlock") => {
                    Some(ins.block_name.clone())
                }
                _ => None,
            })
            .expect("dynamic insert");
        let dyn_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr(&insert_block)
            .unwrap();
        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));

        harness.bedit_save_step(member, 1);

        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));
        assert_eq!(
            keep_before,
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr("SaveOtherBlock_0")
                .unwrap()
        );
        let insert_block_after = harness.0.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|e| match e {
                EntityType::Insert(ins) if !ins.block_name.starts_with("SaveOtherBlock") => {
                    Some(ins.block_name.clone())
                }
                _ => None,
            })
            .expect("dynamic insert");
        assert_ne!(
            Some(dyn_before),
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr(&insert_block_after)
        );
    }

    #[test]
    fn find_replace_preserves_unrelated_blocks_and_top_level_cache() {
        let mut harness = BlockBenchHarness::setup_find_replace(5, 2, 2);
        let i = harness.0.active_tab;
        let top_line = first_top_level_line(&harness.0);
        let keep_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr("FindOtherBlock_0")
            .unwrap();
        let edited_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr("FindBenchBlock")
            .unwrap();
        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));

        harness.find_replace_cycle(0);

        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));
        assert_eq!(
            keep_before,
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr("FindOtherBlock_0")
                .unwrap()
        );
        assert_ne!(
            edited_before,
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr("FindBenchBlock")
                .unwrap()
        );
    }

    #[test]
    fn battman_sync_preserves_unrelated_blocks_and_top_level_cache() {
        let mut harness = BlockBenchHarness::setup_battman(5, 2, 2);
        let i = harness.0.active_tab;
        let top_line = first_top_level_line(&harness.0);
        let keep_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr("AttOtherBlock_0")
            .unwrap();
        let att_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr("AttBenchBlock")
            .unwrap();
        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));

        harness.battman_apply(0);

        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));
        assert_eq!(
            keep_before,
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr("AttOtherBlock_0")
                .unwrap()
        );
        assert_ne!(
            att_before,
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr("AttBenchBlock")
                .unwrap()
        );
    }

    #[test]
    fn bcycleorder_apply_preserves_unrelated_blocks_and_top_level_cache() {
        let mut harness = BlockBenchHarness::setup_bcycleorder(5, 2, 2);
        let i = harness.0.active_tab;
        let top_line = first_top_level_line(&harness.0);
        let keep_before = harness.0.tabs[i]
            .scene
            .active_block_cache_arc()
            .defn_ptr("CycleOtherBlock_0")
            .unwrap();
        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));

        harness.bcycleorder_apply();

        assert!(harness.0.tabs[i].scene.has_resident_tess_memo(top_line));
        assert_eq!(
            keep_before,
            harness.0.tabs[i]
                .scene
                .active_block_cache_arc()
                .defn_ptr("CycleOtherBlock_0")
                .unwrap()
        );
    }
}
