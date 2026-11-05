use super::{Message, OpenCADStudio};
use iced::Task;

impl OpenCADStudio {
    pub(super) fn reset_block_constraint_display(&mut self, i: usize) {
        self.tabs[i].scene.set_block_constraint_display(
            false,
            self.block_editor_preferences.constraint_colors.clone(),
        );
    }

    pub(super) fn sync_block_constraint_colors(&mut self) {
        for tab in &mut self.tabs {
            let enabled = tab.scene.block_constraint_status_mode;
            tab.scene.set_block_constraint_display(
                enabled,
                self.block_editor_preferences.constraint_colors.clone(),
            );
        }
    }

    pub(super) fn dispatch_constraint_status_mode(
        &mut self,
        i: usize,
        value: Option<&str>,
    ) -> Task<Message> {
        if let Some(value) = value {
            let enabled = match value {
                "0" => false,
                "1" => true,
                "TOGGLE" => !self.tabs[i].scene.block_constraint_status_mode,
                _ => {
                    self.command_line
                        .push_error(crate::t!("BCONSTATUSMODE requires 0 or 1.").as_ref());
                    return Task::none();
                }
            };
            self.tabs[i].scene.set_block_constraint_display(
                enabled,
                self.block_editor_preferences.constraint_colors.clone(),
            );
            if self.pending_setvar.as_deref() == Some("BCONSTATUSMODE") {
                self.pending_setvar = None;
            }
            self.command_line
                .push_output(&format!("BCONSTATUSMODE = {}", u8::from(enabled)));
        } else {
            let value = u8::from(self.tabs[i].scene.block_constraint_status_mode);
            self.command_line
                .push_output(&crate::ui::command_line::value_prompt("BCONSTATUSMODE", &value));
            self.pending_setvar = Some("BCONSTATUSMODE".into());
        }
        Task::none()
    }
}
