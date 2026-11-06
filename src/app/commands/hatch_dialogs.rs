//! The boundary warning an empty-area HATCH / GRADIENT pick shows.

use super::*;
use crate::ui::window::hatch_dialogs::{BoundaryWarningState, HatchDialogMsg};

impl OpenCADStudio {
    /// A pick or selection with no closed boundary: the warning window, or
    /// once it is turned off the message; the prompt comes back either way.
    pub(in crate::app) fn hatch_boundary_missed(&mut self, lead: String) {
        for line in lead.lines() {
            self.command_line.push_output(line);
        }
        let i = self.active_tab;
        let gap = self.tabs[i]
            .active_cmd
            .as_ref()
            .and_then(|command| command.hatch_gap_tolerance())
            .unwrap_or(0.0);
        if self.hatch_boundary_warning_off {
            self.command_line
                .push_output(&crate::tr!("hatch-cmd", "no-valid-boundary"));
            self.hatch_reprompt();
            return;
        }
        self.hatch_boundary_warning = Some(BoundaryWarningState {
            gap,
            ..Default::default()
        });
        self.active_modal = Some(crate::app::ModalKind::HatchBoundaryWarning);
    }

    pub(in crate::app) fn update_hatch_dialog(&mut self, message: HatchDialogMsg) -> Task<Message> {
        match message {
            HatchDialogMsg::WarningDetails(on) => {
                if let Some(state) = self.hatch_boundary_warning.as_mut() {
                    state.details = on;
                }
            }
            HatchDialogMsg::WarningDontShow(on) => {
                if let Some(state) = self.hatch_boundary_warning.as_mut() {
                    state.dont_show = on;
                }
            }
            HatchDialogMsg::WarningClose => self.close_active_modal(),
        }
        Task::none()
    }

    /// Closing the boundary warning: keep "Do not show this message again"
    /// and ask the prompt again.
    pub(in crate::app) fn close_hatch_boundary_warning(&mut self) {
        if let Some(state) = self.hatch_boundary_warning.take() {
            if state.dont_show && !self.hatch_boundary_warning_off {
                self.hatch_boundary_warning_off = true;
                self.save_config();
            }
        }
        // After the window the search's verdict is printed, then the prompt.
        self.command_line
            .push_output(&crate::tr!("hatch-cmd", "no-valid-boundary"));
        self.hatch_reprompt();
    }

    /// The running command's prompt and options again.
    pub(in crate::app) fn hatch_reprompt(&mut self) {
        let i = self.active_tab;
        if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
            self.command_line.push_info(&prompt);
        }
        let options = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.options())
            .unwrap_or_default();
        self.command_line.set_step_options(options);
    }
}
