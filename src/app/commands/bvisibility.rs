//! BVSTATE, BVHIDE and BVSHOW: the visibility states of the block open in
//! the Block Editor (see `scene::block_visibility`).

use crate::app::{Message, OpenCADStudio};
use crate::scene::block_visibility as visibility;
use codec::{Document, EntityType, Handle};
use iced::Task;

impl OpenCADStudio {
    pub(in crate::app) fn handle_place_block_orientation_parameter(
        &mut self,
        result: crate::command::CmdResult,
    ) -> Task<Message> {
        let crate::command::CmdResult::PlaceBlockOrientationParameter {
            document_id,
            definition,
            kind,
            perpendicular,
            base,
            end,
            reference,
        } = result
        else {
            unreachable!()
        };
        let i = self.active_tab;
        if self.tabs[i].id != document_id
            || self.tabs[i]
                .active_block_edit_session()
                .map(|s| s.br_handle)
                != Some(definition)
        {
            self.command_line.push_error(
                crate::t!("The parameter's Block Editor session is no longer active.").as_ref(),
            );
            return Task::none();
        }
        let vector = |p: glam::DVec3| codec::types::Vector3::new(p.x, p.y, p.z);
        if let Some(error) = self.parameter_placement_error(kind.name()).or_else(|| {
            crate::scene::block_orientation_parameter::points_error(
                kind,
                vector(base),
                vector(end),
                vector(reference),
            )
        }) {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        if perpendicular && kind != crate::scene::block_orientation_parameter::Kind::Alignment {
            self.command_line.push_error(crate::t!("Alignment type is only available for Alignment parameters.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, &format!("{} parameter", kind.name()));
        let parameter = crate::scene::block_orientation_parameter::create(
            &mut self.tabs[i].scene.document,
            definition,
            kind,
            vector(base),
            vector(end),
            vector(reference),
        )
        .expect("validated parameter placement");
        if perpendicular {
            crate::scene::block_orientation_parameter::set_alignment_type(&mut self.tabs[i].scene.document, definition, parameter, true)
                .expect("validated Alignment parameter placement");
        }
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].scene.clear_preview_wire();
        self.finish_pending_history(i);
        self.refresh_properties();
        self.command_line
            .push_output(crate::tf!("{} parameter placed.", crate::t!(kind.name())).as_ref());
        self.finish_parameter_set_placement(i, parameter)
    }

    pub(in crate::app) fn handle_place_block_distance_parameter(
        &mut self,
        result: crate::command::CmdResult,
    ) -> Task<Message> {
        let crate::command::CmdResult::PlaceBlockDistanceParameter {
            document_id,
            definition,
            kind,
            base,
            end,
            label,
        } = result
        else {
            unreachable!()
        };
        let i = self.active_tab;
        if self.tabs[i].id != document_id
            || self.tabs[i]
                .active_block_edit_session()
                .map(|s| s.br_handle)
                != Some(definition)
        {
            self.command_line.push_error(
                crate::t!("The parameter's Block Editor session is no longer active.").as_ref(),
            );
            return Task::none();
        }
        let vector = |p: glam::DVec3| codec::types::Vector3::new(p.x, p.y, p.z);
        if let Some(error) = self.parameter_placement_error(kind.name()).or_else(|| {
            crate::scene::block_distance_parameter::points_error(
                kind,
                vector(base),
                vector(end),
                vector(label),
            )
        }) {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, &format!("{} parameter", kind.name()));
        let parameter = crate::scene::block_distance_parameter::create(
            &mut self.tabs[i].scene.document,
            definition,
            kind,
            vector(base),
            vector(end),
            vector(label),
        )
        .expect("validated parameter placement");
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].scene.clear_preview_wire();
        self.finish_pending_history(i);
        self.refresh_properties();
        self.command_line
            .push_output(crate::tf!("{} parameter placed.", crate::t!(kind.name())).as_ref());
        self.finish_parameter_set_placement(i, parameter)
    }

    pub(in crate::app) fn point_parameter_placement_error(&self) -> Option<&'static str> {
        self.parameter_placement_error("POINT")
    }

    pub(in crate::app) fn parameter_placement_error(&self, kind: &str) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            return Some("Open a block in the Block Editor before placing a parameter.");
        };
        match kind.to_ascii_uppercase().as_str() {
            "BASEPOINT"|"BASE"|"B" => crate::scene::block_basepoint::placement_error(&tab.scene.document,session.br_handle),
            "LOOKUP"|"K" => crate::scene::block_lookup::placement_error(&tab.scene.document,session.br_handle),
            "POINT" | "P" | "" => crate::scene::block_point_parameter::placement_error(
                &tab.scene.document,
                session.br_handle,
            ),
            "LINEAR" | "L" | "POLAR" | "PO" | "XY" => {
                crate::scene::block_distance_parameter::placement_error(
                    &tab.scene.document,
                    session.br_handle,
                )
            }
            "ROTATION" | "R" | "ALIGNMENT" | "A" | "FLIP" | "F" => crate::scene::block_orientation_parameter::placement_error(&tab.scene.document, session.br_handle, crate::scene::block_orientation_parameter::Kind::parse(kind).unwrap()),
            "VISIBILITY" | "V" => {
                visibility::placement_error(&tab.scene.document, session.br_handle)
            }
            _ => Some("Supported parameter types: Point, Linear, Polar, XY, Rotation, Alignment, Flip, Visibility, Lookup, Basepoint."),
        }
    }

    pub(in crate::app) fn handle_place_block_point_parameter(
        &mut self,
        result: crate::command::CmdResult,
    ) -> Task<Message> {
        let crate::command::CmdResult::PlaceBlockPointParameter {
            document_id,
            definition,
            location,
            label,
        } = result
        else {
            unreachable!()
        };
        let i = self.active_tab;
        if self.tabs[i].id != document_id
            || self.tabs[i]
                .active_block_edit_session()
                .map(|session| session.br_handle)
                != Some(definition)
        {
            self.command_line.push_error(
                crate::t!("The parameter's Block Editor session is no longer active.").as_ref(),
            );
            return Task::none();
        }
        if let Some(error) = self.point_parameter_placement_error() {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        if !location.is_finite() || !label.is_finite() {
            self.command_line
                .push_error(crate::t!("Specify finite parameter and label locations.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, "Point parameter");
        let parameter = crate::scene::block_point_parameter::create(
            &mut self.tabs[i].scene.document,
            definition,
            codec::types::Vector3::new(location.x, location.y, location.z),
            codec::types::Vector3::new(label.x, label.y, label.z),
        )
        .expect("validated Point placement");
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].scene.clear_preview_wire();
        self.finish_pending_history(i);
        self.refresh_properties();
        self.command_line
            .push_output(crate::t!("Point parameter placed.").as_ref());
        self.finish_parameter_set_placement(i, parameter)
    }

    pub(in crate::app) fn visibility_placement_error(&self) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            return Some("Open a block in the Block Editor before placing a parameter.");
        };
        visibility::placement_error(&tab.scene.document, session.br_handle)
    }

    pub(in crate::app) fn handle_place_block_visibility_parameter(
        &mut self,
        document_id: u64,
        definition: Handle,
        location: glam::DVec3,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].id != document_id
            || self.tabs[i]
                .active_block_edit_session()
                .map(|session| session.br_handle)
                != Some(definition)
        {
            self.command_line.push_error(
                crate::t!("The parameter's Block Editor session is no longer active.").as_ref(),
            );
            return Task::none();
        }
        if let Some(error) = self.visibility_placement_error() {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        if !location.is_finite() {
            self.command_line
                .push_error(crate::t!("Specify a finite parameter location.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, "Visibility parameter");
        let document = &mut self.tabs[i].scene.document;
        let parameter = visibility::create(
            document,
            definition,
            codec::types::Vector3::new(location.x, location.y, location.z),
        )
        .expect("validated visibility placement");
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.finish_pending_history(i);
        self.refresh_properties();
        self.command_line.push_output(
            crate::t!("Visibility parameter placed. Use BVSTATE to manage its states.").as_ref(),
        );
        self.finish_parameter_set_placement(i, parameter)
    }

    /// The block open in the Block Editor, or an error naming `command`.
    fn visibility_block(&mut self, i: usize, command: &str) -> Option<Handle> {
        let block = self.tabs[i]
            .active_block_edit_session()
            .map(|session| session.br_handle);
        if block.is_none() {
            self.command_line.push_error(
                crate::tf!(
                    "{}: open the block in the Block Editor first (BEDIT).",
                    command
                )
                .as_ref(),
            );
        }
        block
    }

    /// The block's visibility parameter, made when it has none: its grip
    /// sits on the block's base point.
    fn visibility_parameter(&mut self, i: usize, block: Handle) -> Option<Handle> {
        let document = &mut self.tabs[i].scene.document;
        let base = document
            .block_records
            .iter()
            .find(|record| record.handle == block)
            .and_then(
                |record| match document.get_entity(record.block_entity_handle) {
                    Some(EntityType::Block(begin)) => Some(begin.base_point),
                    _ => None,
                },
            )
            .unwrap_or(codec::types::Vector3::ZERO);
        visibility::create(document, block, base)
    }

    /// The state the block shows in the editor.
    pub(in crate::app) fn visibility_current(&self, i: usize, block: Handle) -> usize {
        let scene = &self.tabs[i].scene;
        scene
            .visibility_states
            .get(&block)
            .copied()
            .filter(|index| {
                visibility::parameter(&scene.document, block)
                    .is_some_and(|parameter| *index < parameter.states.len())
            })
            .unwrap_or_else(|| {
                visibility::parameter(&scene.document, block).map_or(0, |parameter| {
                    visibility::shown_state(&scene.document, parameter.handle)
                })
            })
    }

    pub(in crate::app) fn show_visibility_state(
        &mut self,
        i: usize,
        block: Handle,
        parameter: Handle,
        state: usize,
    ) {
        visibility::apply_state(&mut self.tabs[i].scene.document, parameter, state);
        self.tabs[i].scene.visibility_states.insert(block, state);
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        if let Some(name) =
            visibility::state_names(&self.tabs[i].scene.document, parameter).get(state)
        {
            self.command_line
                .push_output(crate::tf!("Current visibility state: {}", name).as_ref());
        }
    }

    pub(super) fn dispatch_bvisibility(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        use crate::command::CadCommand;
        use crate::modules::draw::modify::bvstate::{BvStateCommand, SEP};
        if matches!(cmd, "BVSTATE" | "BVHIDE" | "BVSHOW" | "_BVHIDE" | "_BVSHOW")
            || cmd.starts_with("_BVSTATE")
        {
            if let Some(block) = self.tabs[i]
                .active_block_edit_session()
                .map(|session| session.br_handle)
            {
                if visibility::parameter(&self.tabs[i].scene.document, block).is_none() {
                    if let Some(error) =
                        visibility::placement_error(&self.tabs[i].scene.document, block)
                    {
                        self.command_line.push_error(crate::t!(error).as_ref());
                        return Some(Task::none());
                    }
                }
            }
        }
        let variable = cmd.strip_prefix("SETVAR ").unwrap_or(cmd);
        if variable == "BVMODE" || variable.starts_with("BVMODE ") {
            let value = variable.strip_prefix("BVMODE").unwrap().trim();
            // Inline command probing may have opened the bare variable prompt.
            if !value.is_empty() && self.pending_setvar.as_deref() == Some("BVMODE") {
                self.pending_setvar = None;
            }
            match value {
                "" => {
                    self.command_line.push_output(
                        crate::tf!(
                            "BVMODE: enter 0 (hide) or 1 (show dimmed) <{}>:",
                            u8::from(self.block_visibility_mode)
                        )
                        .as_ref(),
                    );
                    self.pending_setvar = Some("BVMODE".into());
                }
                "TOGGLE" => {
                    self.block_visibility_mode = !self.block_visibility_mode;
                    self.sync_block_visibility_display();
                }
                value @ ("0" | "1") => {
                    self.block_visibility_mode = value == "1";
                    self.sync_block_visibility_display();
                    self.command_line
                        .push_output(crate::tf!("BVMODE = {}", value).as_ref());
                }
                _ => self
                    .command_line
                    .push_error(crate::t!("BVMODE: only 0 or 1 is allowed.").as_ref()),
            }
            return Some(Task::none());
        }
        match cmd {
            cmd if cmd == "BPARAMETERSET" || cmd.starts_with("BPARAMETERSET ") => {
                let label = cmd.strip_prefix("BPARAMETERSET").unwrap().trim();
                let Some(set) = crate::modules::block_editor::parameter_set::get(label) else {
                    self.command_line
                        .push_error(crate::t!("This Parameter Set is not supported yet.").as_ref());
                    return Some(Task::none());
                };
                if let Some(error) = self.parameter_placement_error(set.parameter) {
                    self.command_line.push_error(crate::t!(error).as_ref());
                    return Some(Task::none());
                }
                self.tabs[i].pending_parameter_set_follow_up = Some(set.follow_up);
                Some(self.dispatch_command(&format!("BPARAMETER {}", set.parameter)))
            }
            "BACTIONSET" => Some(self.start_block_action_set()),
            "BASSOCIATE" => Some(self.start_block_action_association()),
            cmd if cmd=="BACTIONBAR" || cmd.starts_with("BACTIONBAR ") => Some(self.start_action_bar_command(cmd.strip_prefix("BACTIONBAR").unwrap().trim())),
            cmd if cmd == "BACTION"
                || cmd.starts_with("BACTION ")
                || cmd == "BACTIONTOOL"
                || cmd.starts_with("BACTIONTOOL ")
                || cmd == "BARRAY"
                || cmd.starts_with("BARRAY ") =>
            {
                Some(
                    self.start_block_action(
                        if cmd == "BARRAY" || cmd.starts_with("BARRAY ") {
                            "ARRAY"
                        } else {
                            cmd.split_once(' ').map_or("", |(_, args)| args.trim())
                        },
                    ),
                )
            }
            cmd if cmd == "BLOOKUPTABLE" || cmd.starts_with("BLOOKUPTABLE ") => {
                Some(self.open_lookup_table(cmd.strip_prefix("BLOOKUPTABLE").unwrap().trim()))
            }
            cmd if cmd == "BTABLE"
                || cmd == "BPROPERTIESTABLE"
                || cmd.starts_with("BPROPERTIESTABLE ") => {
                Some(self.open_block_properties_table())
            }
            "RESETBLOCK" | "RESETBLOCK APPLY" => {
                Some(self.reset_selected_block_references(cmd == "RESETBLOCK APPLY"))
            }
            cmd if cmd == "BPARAMETER" || cmd.starts_with("BPARAMETER ") => {
                let Some(block) = self.visibility_block(i, "BPARAMETER") else {
                    return Some(Task::none());
                };
                let parameter_type = cmd.strip_prefix("BPARAMETER").unwrap().trim();
                if let Some(kind) =
                    crate::modules::block_editor::single_parameter::Kind::parse(parameter_type)
                {
                    let command =
                        crate::modules::block_editor::single_parameter::SingleParameterCommand::new(
                            self.tabs[i].id,
                            block,
                            kind,
                        );
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                    return Some(Task::none());
                }
                if let Some(kind) =
                    crate::scene::block_orientation_parameter::Kind::parse(parameter_type)
                {
                    let command = crate::modules::block_editor::orientation_parameter::OrientationParameterCommand::new(self.tabs[i].id,block,kind);
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                    return Some(Task::none());
                }
                if let Some(kind) =
                    crate::scene::block_distance_parameter::Kind::parse(parameter_type)
                {
                    let command = crate::modules::block_editor::distance_parameter::DistanceParameterCommand::new(self.tabs[i].id, block, kind);
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                    return Some(Task::none());
                }
                if parameter_type.eq_ignore_ascii_case("POINT")
                    || parameter_type.eq_ignore_ascii_case("P")
                {
                    let command =
                        crate::modules::block_editor::point_parameter::PointParameterCommand::new(
                            self.tabs[i].id,
                            block,
                        );
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                    return Some(Task::none());
                }
                if !parameter_type.is_empty()
                    && !parameter_type.eq_ignore_ascii_case("VISIBILITY")
                    && !parameter_type.eq_ignore_ascii_case("V")
                {
                    self.command_line.push_error(
                        crate::t!(
                            "Supported parameter types: Point, Linear, Polar, XY, Rotation, Alignment, Flip, Visibility, Lookup, Basepoint."
                        )
                        .as_ref(),
                    );
                    return Some(Task::none());
                }
                let command = crate::modules::block_editor::visibility_parameter::VisibilityParameterCommand::new(self.tabs[i].id, block, parameter_type.is_empty());
                self.command_line.push_info(&command.prompt());
                self.command_line.set_step_options(command.options());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            "BVSTATE DIALOG" => {
                if let Some(context) = self.block_visibility_context() {
                    return Some(self.handle_block_visibility(
                        crate::ui::window::block_visibility::Action::Open(context.target),
                    ));
                }
                self.command_line.push_error(
                    crate::t!("Place a Visibility parameter in the Block Editor first.").as_ref(),
                );
                Some(Task::none())
            }
            "BVSTATE" => {
                let block = self.visibility_block(i, "BVSTATE")?;
                let document = &self.tabs[i].scene.document;
                let names = visibility::parameter(document, block)
                    .map(|parameter| visibility::state_names(document, parameter.handle))
                    .unwrap_or_else(|| vec!["VisibilityState0".to_string()]);
                let current = self.visibility_current(i, block);
                if let Some(name) = names.get(current) {
                    self.command_line
                        .push_output(crate::tf!("Current visibility state: {}", name).as_ref());
                }
                let command = BvStateCommand::new(names, current);
                self.command_line.push_info(&command.prompt());
                self.command_line.set_step_options(command.options());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            cmd if cmd.starts_with("_BVSTATE") => {
                let block = self.visibility_block(i, "BVSTATE")?;
                let fields: Vec<&str> = cmd.split(SEP).skip(1).collect();
                self.push_undo_snapshot(i, "BVSTATE");
                let parameter = self.visibility_parameter(i, block)?;
                let current = self.visibility_current(i, block);
                let document = &mut self.tabs[i].scene.document;
                let index = |document: &Document, name: &str| {
                    visibility::state_names(document, parameter)
                        .iter()
                        .position(|candidate| candidate.eq_ignore_ascii_case(name))
                };
                let shown = match fields.as_slice() {
                    ["NEW", name] => visibility::add_state(document, parameter, name, current),
                    ["SET", name] => index(document, name),
                    ["RENAME", from, to] => {
                        if let Some(state) = index(document, from) {
                            visibility::rename_state(document, parameter, state, to);
                        }
                        Some(current)
                    }
                    ["DELETE", name] => index(document, name).map(|state| {
                        visibility::delete_state(document, parameter, state);
                        let count = visibility::state_names(document, parameter).len();
                        if current == state || current >= count {
                            0
                        } else if current > state {
                            current - 1
                        } else {
                            current
                        }
                    }),
                    _ => None,
                };
                self.show_visibility_state(i, block, parameter, shown.unwrap_or(current));
                self.tabs[i].active_cmd = None;
                Some(Task::none())
            }
            "BVHIDE" | "BVSHOW" => {
                self.visibility_block(i, cmd)?;
                let hide = cmd == "BVHIDE";
                let pending = if hide { "_BVHIDE" } else { "_BVSHOW" };
                if !self.tabs[i].scene.selected.is_empty() {
                    return Some(self.dispatch_command(pending));
                }
                let prompt = if hide {
                    "Select objects to hide:"
                } else {
                    "Select objects to make visible:"
                };
                use crate::modules::draw::select::SelectObjectsCommand;
                let command = SelectObjectsCommand::with_prompt(cmd, pending, prompt);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                Some(Task::none())
            }
            "_BVHIDE" | "_BVSHOW" => {
                let block = self.visibility_block(i, &cmd[1..])?;
                let handles: Vec<Handle> = self.tabs[i]
                    .scene
                    .selected_handles_in_order()
                    .into_iter()
                    .filter(|handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(*handle)
                            .is_some_and(|entity| entity.common().owner_handle == block)
                    })
                    .collect();
                self.push_undo_snapshot(i, &cmd[1..]);
                let parameter = self.visibility_parameter(i, block)?;
                let current = self.visibility_current(i, block);
                visibility::set_visible(
                    &mut self.tabs[i].scene.document,
                    parameter,
                    current,
                    &handles,
                    cmd == "_BVSHOW",
                );
                self.tabs[i].scene.deselect_all();
                self.show_visibility_state(i, block, parameter, current);
                self.tabs[i].active_cmd = None;
                Some(Task::none())
            }
            _ => None,
        }
    }

    pub(in crate::app) fn finish_parameter_set_placement(
        &mut self,
        i: usize,
        parameter: Handle,
    ) -> Task<Message> {
        match self.tabs[i].pending_parameter_set_follow_up.take() {
            Some(follow_up @ ("XY_MOVE_BOX_SET" | "XY_STRETCH_BOX_SET")) => {
                let kind = if follow_up == "XY_MOVE_BOX_SET" {
                    crate::scene::block_actions::Kind::Move
                } else {
                    crate::scene::block_actions::Kind::Stretch
                };
                self.tabs[i].pending_xy_box_set = Some((parameter, kind, 0));
                self.start_xy_box_set_action(parameter, kind, 0)
            }
            Some(follow_up @ ("MOVE_PAIR" | "STRETCH_PAIR")) => {
                let kind = if follow_up == "MOVE_PAIR" {
                    crate::scene::block_actions::Kind::Move
                } else {
                    crate::scene::block_actions::Kind::Stretch
                };
                self.tabs[i].pending_action_pair = Some((parameter, kind, 0));
                self.start_parameter_pair_action(parameter, kind, 0)
            }
            Some(command) if command.starts_with("BACTION ") => {
                self.start_block_action_for_parameter(
                    command.trim_start_matches("BACTION "),
                    parameter,
                )
            }
            Some(command) => self.dispatch_command(command),
            None => Task::none(),
        }
    }
}
