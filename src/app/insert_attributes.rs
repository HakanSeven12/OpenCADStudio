//! The insertion-time attribute form: `ATTDIA` answers the whole attribute list
//! at once, and the answers go back through the parked insert command.
use super::{Message, ModalKind, OpenCADStudio};
use crate::ui::window::insert_attr_dialog::{
    AttrFormEdit, InsertAttrMsg, InsertAttrRow, InsertAttrState,
};
use document::entities::AttributeDefinition;
use iced::Task;

impl OpenCADStudio {
    /// Ask for every attribute the operator has to answer, prefilled with the
    /// defaults the prompt chain would have offered.
    pub(super) fn open_insert_attr_dialog(
        &mut self,
        block_name: &str,
        attdefs: &[AttributeDefinition],
    ) {
        self.insert_attr_dialog = Some(InsertAttrState {
            block_name: block_name.to_owned(),
            rows: attdefs
                .iter()
                .map(|ad| InsertAttrRow {
                    tag: ad.tag.clone(),
                    label: if ad.prompt.is_empty() {
                        ad.tag.clone()
                    } else {
                        ad.prompt.clone()
                    },
                    value: ad.default_value.clone(),
                })
                .collect(),
            first: 0,
            edit: None,
        });
        self.active_modal = Some(ModalKind::InsertAttributes);
    }

    /// ATTEDIT: the form with a block reference's current values, every
    /// attribute but the constant ones, each under its prompt.
    pub(in crate::app) fn open_attribute_value_form(&mut self, handle: document::Handle) {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let Some(document::EntityType::Insert(ins)) = doc.get_entity(handle) else {
            return;
        };
        let meta = crate::app::update::command::block_attr_meta(doc, &ins.block_name);
        let mut rows = Vec::new();
        let mut indices = Vec::new();
        let mut multiline = Vec::new();
        for (index, a) in ins.attributes.iter().enumerate() {
            let def = meta.get(&a.tag);
            if a.flags.constant || def.is_some_and(|m| m.constant) {
                continue;
            }
            let prompt = def.map(|m| m.prompt.clone()).unwrap_or_default();
            rows.push(InsertAttrRow {
                tag: a.tag.clone(),
                label: if prompt.is_empty() { a.tag.clone() } else { prompt },
                value: a.get_value().to_string(),
            });
            indices.push(index);
            multiline.push(a.is_multiline || a.embedded_mtext.is_some());
        }
        self.insert_attr_dialog = Some(InsertAttrState {
            block_name: ins.block_name.clone(),
            rows,
            first: 0,
            edit: Some(AttrFormEdit {
                handle,
                indices,
                multiline,
                editing: None,
            }),
        });
        self.active_modal = Some(ModalKind::InsertAttributes);
    }

    /// ATTEDIT's OK: the values go onto the block reference as one step.
    fn apply_attribute_value_form(
        &mut self,
        handle: document::Handle,
        indices: Vec<usize>,
        rows: Vec<InsertAttrRow>,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.push_undo_snapshot(i, "ATTEDIT");
        let mut changed = false;
        if let Some(document::EntityType::Insert(ins)) =
            self.tabs[i].scene.document.get_entity_mut(handle)
        {
            for (index, row) in indices.into_iter().zip(rows) {
                if let Some(a) = ins.attributes.get_mut(index) {
                    if a.get_value() != row.value {
                        a.set_value(row.value);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.tabs[i]
                .scene
                .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
        } else {
            self.discard_last_undo_entry(i);
        }
        self.refresh_properties();
        Task::none()
    }

    /// Feed the answers back through the command that is still parked on the
    /// attribute step, so the attributes are built in exactly one place.
    pub(super) fn apply_insert_attr_dialog(&mut self) -> Task<Message> {
        let Some(dialog) = self.insert_attr_dialog.take() else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        if let Some(edit) = dialog.edit {
            return self.apply_attribute_value_form(edit.handle, edit.indices, dialog.rows);
        }
        // The form is modal, so the tab that parked the placement is still the
        // active one by the time an answer comes back.
        let i = self.active_tab;
        let mut last = None;
        if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
            // The form answered every attribute, verified ones included.
            cmd.attreq_from_dialog();
        }
        for row in &dialog.rows {
            if let Some(result) = self.tabs[i]
                .active_cmd
                .as_mut()
                .and_then(|cmd| cmd.on_text_input(&row.value))
            {
                last = Some(result);
            }
        }
        let Some(result) = last else {
            self.cancel_insert_attr_command();
            return Task::none();
        };
        self.apply_cmd_result(result)
    }

    /// Cancel on a form that only declines the values: every attribute keeps
    /// its default and the command goes on.
    pub(super) fn keep_insert_attr_defaults(&mut self) -> Task<Message> {
        if let Some(dialog) = self.insert_attr_dialog.as_mut() {
            for row in &mut dialog.rows {
                row.value.clear();
            }
        }
        self.apply_insert_attr_dialog()
    }

    /// Drop the parked insert without committing it.
    pub(super) fn cancel_insert_attr_dialog(&mut self) {
        self.insert_attr_dialog = None;
        self.cancel_insert_attr_command();
    }

    fn cancel_insert_attr_command(&mut self) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.command_line
            .push_info(crate::t!("Command: *Cancel*").as_ref());
    }

    pub(super) fn on_insert_attr_dialog(&mut self, message: InsertAttrMsg) -> Task<Message> {
        match message {
            InsertAttrMsg::Value(index, value) => {
                if let Some(dialog) = self.insert_attr_dialog.as_mut() {
                    if let Some(row) = dialog.rows.get_mut(index) {
                        row.value = value;
                    }
                }
                Task::none()
            }
            InsertAttrMsg::EditValue(index) => {
                // The form waits while the text editor types the value.
                let Some(dialog) = self.insert_attr_dialog.as_mut() else {
                    return Task::none();
                };
                let Some(edit) = dialog.edit.as_mut() else {
                    return Task::none();
                };
                edit.editing = Some(index);
                let initial = dialog
                    .rows
                    .get(index)
                    .map(|r| r.value.clone())
                    .unwrap_or_default();
                let i = self.active_tab;
                let pos = self.tabs[i].scene.camera.borrow().target;
                self.active_modal = None;
                self.tabs[i].active_cmd = Some(Box::new(
                    crate::app::attr_editor::AttrEditorValueCommand::new("_ATTEDIT_FORM_VALUE"),
                ));
                self.apply_cmd_result(crate::command::CmdResult::SuspendForMTextInput {
                    pos,
                    initial,
                    height: 2.5,
                })
            }
            InsertAttrMsg::Previous | InsertAttrMsg::Next => {
                if let Some(dialog) = self.insert_attr_dialog.as_mut() {
                    if matches!(message, InsertAttrMsg::Next) {
                        dialog.next_page();
                    } else {
                        dialog.previous_page();
                    }
                }
                Task::none()
            }
            InsertAttrMsg::Ok => self.apply_insert_attr_dialog(),
            InsertAttrMsg::Help => {
                self.command_line.push_info(
                    crate::t!("Each value entered here becomes the value of the attribute on the block being placed.")
                        .as_ref(),
                );
                Task::none()
            }
        }
    }

    /// `_ATTEDIT_FORM_VALUE`: the text editor closed; the form comes back.
    pub(in crate::app) fn attribute_form_value_back(&mut self) {
        let value = crate::app::attr_editor::take_editor_text();
        if let Some(dialog) = self.insert_attr_dialog.as_mut() {
            if let Some(index) = dialog.edit.as_mut().and_then(|e| e.editing.take()) {
                if let (Some(value), Some(row)) = (value, dialog.rows.get_mut(index)) {
                    row.value = value;
                }
            }
            self.active_modal = Some(ModalKind::InsertAttributes);
        }
    }
}
