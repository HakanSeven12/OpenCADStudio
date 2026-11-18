//! The Password Protected PDF window's state changes: the password checks,
//! what runs once the file is unlocked and the messages when it is not.

use crate::app::{Message, OpenCADStudio};
use crate::scene::model::pdf_password::{self, Check, Stage};
use crate::ui::window::pdf_password_dialog::{PdfPasswordMsg, PdfPasswordState, PdfPasswordThen};
use iced::Task;

impl OpenCADStudio {
    /// Open the window on `path` at `stage`; `remembered` prints the notice
    /// of the password supplied earlier that opened the document.
    pub(in crate::app) fn open_pdf_password(
        &mut self,
        path: &str,
        stage: Stage,
        remembered: bool,
        then: PdfPasswordThen,
    ) -> Task<Message> {
        if remembered {
            self.command_line
                .push_output(&pdf_password::remembered_notice(path));
        }
        self.pdf_password = Some(PdfPasswordState::new(path, stage, then));
        self.active_modal = Some(crate::app::ModalKind::PdfPassword);
        self.focus_pdf_password()
    }

    fn focus_pdf_password(&self) -> Task<Message> {
        iced::widget::operation::focus(iced::widget::Id::new(
            crate::ui::window::pdf_password_dialog::INPUT_ID,
        ))
    }

    pub(in crate::app) fn update_pdf_password(&mut self, message: PdfPasswordMsg) -> Task<Message> {
        let Some(state) = self.pdf_password.as_mut() else {
            return Task::none();
        };
        match message {
            PdfPasswordMsg::Input(value) => state.input = value,
            PdfPasswordMsg::Show(on) => state.show = on,
            PdfPasswordMsg::Remember(on) => state.remember = on,
            PdfPasswordMsg::Help => {
                self.command_line
                    .push_info(&crate::tr!("pdf-password", "help"));
            }
            PdfPasswordMsg::Ok => {
                if state.input.is_empty() {
                    return Task::none();
                }
                let owner = !state.then.attach();
                let (path, stage, remember) = (&state.path, state.stage, state.remember);
                match pdf_password::check(path, stage, &state.input, remember, owner) {
                    Check::Wrong => state.wrong = true,
                    Check::NeedPermission => {
                        state.stage = Stage::Permission;
                        state.input.clear();
                        state.wrong = false;
                        return self.focus_pdf_password();
                    }
                    Check::Unlocked => {
                        let Some(state) = self.pdf_password.take() else {
                            return Task::none();
                        };
                        self.close_active_modal();
                        let focus = self.focus_cmd_input();
                        let next = self.after_pdf_unlocked(state.path, state.then);
                        return Task::batch([focus, next]);
                    }
                }
            }
        }
        Task::none()
    }

    fn after_pdf_unlocked(&mut self, path: String, then: PdfPasswordThen) -> Task<Message> {
        match then {
            PdfPasswordThen::Command { input, .. } => {
                let i = self.active_tab;
                if let Some(result) = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .and_then(|c| c.on_text_input(&input))
                {
                    return self.apply_cmd_result(result);
                }
                Task::none()
            }
            PdfPasswordThen::Attach => self.open_pdf_attach_dialog(&path),
            PdfPasswordThen::ImportFile => self.open_pdf_import_file(&path),
        }
    }

    /// Cancel: the file is refused and a file prompt asks again; attaching
    /// says why, importing asks again silently.
    pub(in crate::app) fn cancel_pdf_password(&mut self) -> Task<Message> {
        let Some(state) = self.pdf_password.take() else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        if state.then.attach() {
            self.command_line
                .push_output(&pdf_password::refused_notice(&state.path));
        }
        if matches!(state.then, PdfPasswordThen::Command { .. }) {
            let i = self.active_tab;
            // The rest of a script line was for this file: it goes with
            // the refusal, as at an Escape after a PAUSE.
            self.tabs[i].pending_pause_tokens = None;
            if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt()) {
                self.command_line.push_info(&prompt);
            }
        }
        self.focus_cmd_input()
    }
}
