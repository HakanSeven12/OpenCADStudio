//! The insertion-time attribute form: `ATTDIA` answers the whole attribute list
//! at once, and the answers go back through the parked insert command.
use super::{Message, ModalKind, OpenCADStudio};
use crate::ui::window::insert_attr_dialog::{InsertAttrMsg, InsertAttrRow, InsertAttrState};
use codec::entities::AttributeDefinition;
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
        });
        self.active_modal = Some(ModalKind::InsertAttributes);
    }

    /// Feed the answers back through the command that is still parked on the
    /// attribute step, so the attributes are built in exactly one place.
    pub(super) fn apply_insert_attr_dialog(&mut self) -> Task<Message> {
        let Some(dialog) = self.insert_attr_dialog.take() else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        // The form is modal, so the tab that parked the placement is still the
        // active one by the time an answer comes back.
        let i = self.active_tab;
        let mut last = None;
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
}
