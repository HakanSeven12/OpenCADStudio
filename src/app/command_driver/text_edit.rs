use super::*;

impl OpenCADStudio {
    pub(super) fn handle_open_mtext_editor(&mut self, result: CmdResult) {
        let i = self.active_tab;
        let CmdResult::OpenMTextEditor {
            pos,
            handle,
            initial,
            height,
            template,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.open_mtext_editor(pos, handle, &initial, height, template.map(|m| *m));
    }

    pub(super) fn handle_suspend_for_mtext_input(&mut self, pos: glam::DVec3, initial: String, height: f64) {
        let i = self.active_tab;
        self.tabs[i].suspended_cmd = self.tabs[i].active_cmd.take();
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.command_mtext_input = true;
        self.pending_command_editor_text = None;
        self.open_mtext_editor(pos, None, &initial, height, None);
    }

    pub(super) fn handle_open_text_editor(&mut self, result: CmdResult) {
        let i = self.active_tab;
        let CmdResult::OpenTextEditor {
            pos,
            handle,
            initial,
            height,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.open_text_inline(
            pos,
            handle,
            &initial,
            height,
            super::super::text_inline::TextEntityField::Text,
            None,
        );
    }

    pub(super) fn handle_suspend_for_text_input(
        &mut self,
        pos: glam::DVec3,
        entity: codec::entities::Text,
    ) {
        let i = self.active_tab;
        self.tabs[i].suspended_cmd = self.tabs[i].active_cmd.take();
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        let height = entity.height;
        self.open_text_inline(
            pos,
            None,
            "",
            height,
            super::super::text_inline::TextEntityField::Text,
            Some(entity),
        );
    }

    pub(super) fn handle_edit_text_entity(&mut self, handle: Handle) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.ribbon.deactivate_tool();
        self.begin_text_edit(handle)
    }

    pub(super) fn handle_suspend_for_text_edit(&mut self, handle: Handle) -> Task<Message> {
        let i = self.active_tab;
        let is_editable =
            crate::app::text_inline::can_edit_text(handle, &self.tabs[i].scene.document);
        if !is_editable {
            self.command_line
                .push_error(crate::t!("TEXTEDIT: selected entity is not text.").as_ref());
            let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
            if let Some(p) = prompt {
                self.command_line.push_info(&p);
            }
            return Task::none();
        }
        let cmd = self.tabs[i].active_cmd.take();
        self.tabs[i].suspended_cmd = cmd;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.ribbon.deactivate_tool();
        self.begin_text_edit(handle)
    }

    pub(super) fn handle_set_textedit_mode(&mut self, val: bool) {
        let i = self.active_tab;
        self.texteditmode = val;
        let display_val = if val { 1 } else { 0 };
        self.command_line
            .push_output(crate::tf!("TEXTEDITMODE set to {display_val}").as_ref());
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
    }

    pub(super) fn handle_edit_dimensions(
        &mut self,
        mut handles: Vec<Handle>,
        operation: crate::command::DimensionEditOperation,
    ) {
        let i = self.active_tab;
        handles.retain(|handle| {
            !self.tabs[i].scene.is_layer_locked(*handle)
                && matches!(
                    self.tabs[i].scene.document.get_entity(*handle),
                    Some(codec::EntityType::Dimension(_))
                )
        });
        if handles.is_empty() {
            self.command_line
                .push_error(crate::t!("DIMEDIT: no editable dimensions were selected.").as_ref());
        } else {
            self.push_undo_snapshot(i, "DIMEDIT");
            for handle in &handles {
                if let Some(codec::EntityType::Dimension(dimension)) =
                    self.tabs[i].scene.document.get_entity_mut(*handle)
                {
                    use crate::command::DimensionEditOperation as Operation;
                    match &operation {
                        Operation::Home => {
                            let base = dimension.base_mut();
                            base.text_middle_point = codec::types::Vector3::ZERO;
                            base.insertion_point = codec::types::Vector3::ZERO;
                            base.text_user_positioned = false;
                            base.text_rotation = 0.0;
                        }
                        Operation::NewText(text) => {
                            dimension
                                .base_mut()
                                .set_text_override((!text.is_empty()).then_some(text.clone()));
                        }
                        Operation::Rotate(degrees) => {
                            dimension.base_mut().text_rotation = degrees.to_radians();
                        }
                        Operation::Oblique(degrees) => match dimension {
                            codec::entities::Dimension::Linear(value) => {
                                value.ext_line_rotation = degrees.to_radians();
                            }
                            codec::entities::Dimension::Aligned(value) => {
                                value.ext_line_rotation = degrees.to_radians();
                            }
                            _ => {}
                        },
                    }
                }
                self.tabs[i].scene.invalidate_dim_block_recorded(*handle);
            }
            let changes: Vec<_> = handles
                .iter()
                .copied()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities(&changes);
            self.tabs[i].dirty = true;
            self.command_line.push_output(
                crate::tf!("DIMEDIT: updated {} dimension(s).", handles.len()).as_ref(),
            );
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
    }

    pub(super) fn handle_ddedit_entity(&mut self, handle: Handle, new_text: String) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            self.tabs[i].active_cmd = None;
            return Some(Task::none());
        }
        self.push_undo_snapshot(i, "DDEDIT");
        let mut updated = false;
        let mut is_dim = false;
        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
            match entity {
                codec::EntityType::Text(t) => {
                    t.value = new_text;
                    updated = true;
                }
                codec::EntityType::MText(t) => {
                    t.value = new_text;
                    updated = true;
                }
                codec::EntityType::AttributeDefinition(a) => {
                    a.default_value = new_text;
                    updated = true;
                }
                codec::EntityType::AttributeEntity(a) => {
                    a.set_value(new_text);
                    updated = true;
                }
                codec::EntityType::Dimension(d) => {
                    // Empty string resets to auto-measured value; otherwise set override.
                    let base = d.base_mut();
                    base.text = new_text;
                    updated = true;
                    is_dim = true;
                }
                _ => {}
            }
        }
        if is_dim {
            // The edited override changed the dimension text; drop its
            // stale *D block so save re-bakes it. (#181)
            self.tabs[i].scene.invalidate_dim_block_recorded(handle);
        }
        if updated {
            self.tabs[i].dirty = true;
            self.command_line
                .push_output(crate::t!("DDEDIT: text updated.").as_ref());
        } else {
            self.discard_last_undo_entry(i);
            self.command_line
                .push_error(crate::t!("DDEDIT: entity type not supported.").as_ref());
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        None
    }
}
