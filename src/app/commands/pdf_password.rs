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
                return self.check_pdf_password_with(pdf_password::check);
            }
            PdfPasswordMsg::Checked(path, stage, input, check) => {
                return self.finish_pdf_password_check(&path, stage, &input, check);
            }
        }
        Task::none()
    }

    /// OK in the password window: the password is checked off the UI
    /// thread (the check reads the file's encryption dictionary and derives
    /// its keys), and the window takes the answer when the check ends
    /// ([`Self::finish_pdf_password_check`]). A panic in the check becomes
    /// an error on the command line, and the window stays open.
    pub(in crate::app) fn check_pdf_password_with(
        &mut self,
        check: fn(&str, Stage, &str, bool, bool) -> Check,
    ) -> Task<Message> {
        let Some(state) = self.pdf_password.as_ref() else {
            return Task::none();
        };
        let owner = !state.then.attach();
        let context = if state.then.attach() {
            "PDFATTACH"
        } else {
            "PDFIMPORT"
        };
        let (path, stage, input, remember) = (
            state.path.clone(),
            state.stage,
            state.input.clone(),
            state.remember,
        );
        let (worker_path, worker_input) = (path.clone(), input.clone());
        crate::app::update::background_task(
            context,
            move || check(&worker_path, stage, &worker_input, remember, owner),
            move |answer| Message::PdfPassword(PdfPasswordMsg::Checked(path, stage, input, answer)),
        )
    }

    /// The end of a password check: the window takes the answer if it is
    /// still open on that file and stage with that password typed.
    fn finish_pdf_password_check(
        &mut self,
        path: &str,
        stage: Stage,
        input: &str,
        check: Check,
    ) -> Task<Message> {
        let Some(state) = self.pdf_password.as_mut() else {
            return Task::none();
        };
        if state.path != path || state.stage != stage || state.input != input {
            return Task::none();
        }
        match check {
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

#[cfg(test)]
mod password_check_tests {
    use super::*;
    use crate::app::ModalKind;
    use std::sync::Arc;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// The password window for an encrypted file (PDFIMPORT's window read),
    /// with `typed` in its field.
    fn password_window(app: &mut OpenCADStudio, path: &str, typed: &str) {
        let file = "%PDF-1.4\ntrailer\n<< /Encrypt << /Filter /Standard /V 1 /R 2 \
            /O <95fe1f4219662a774442fb072e3d9f19e9d130ec09a4d0061e78fe920f7ab62f> \
            /U <0102030405060708090a0b0c0d0e0f100102030405060708090a0b0c0d0e0f10> /P -44 >> >>\n%%EOF\n";
        crate::scene::model::pdf_raster::register_source(path, Arc::new(file.as_bytes().to_vec()));
        let read = app.open_pdf_import_file(path);
        app.drive_headless_task(read).expect("the read task");
        assert_eq!(app.active_modal, Some(ModalKind::PdfPassword));
        let _ = app.update(Message::PdfPassword(PdfPasswordMsg::Input(
            typed.to_string(),
        )));
    }

    fn wrong(app: &OpenCADStudio) -> bool {
        app.pdf_password.as_ref().is_some_and(|state| state.wrong)
    }

    /// OK checks the password on a worker: the window waits, and says the
    /// password is wrong when the check ends.
    #[test]
    fn ok_checks_the_password_on_a_worker() {
        let mut app = new_app();
        password_window(&mut app, "password-on-a-worker.pdf", "not it");
        let check = app.update(Message::PdfPassword(PdfPasswordMsg::Ok));
        assert!(!wrong(&app), "no answer during the check");
        app.drive_headless_task(check).expect("the check");
        assert!(wrong(&app));
        assert_eq!(app.active_modal, Some(ModalKind::PdfPassword));
    }

    /// A check that panics gives an error, and the window stays open.
    #[test]
    fn a_check_that_panics_keeps_the_window() {
        let mut app = new_app();
        password_window(&mut app, "password-panics.pdf", "x");
        let check = app.check_pdf_password_with(|_, _, _, _, _| panic!("a reader bug"));
        app.drive_headless_task(check).expect("the check");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("PDFIMPORT failed: a reader bug")
        );
        assert_eq!(app.active_modal, Some(ModalKind::PdfPassword));
        assert!(!wrong(&app));
    }

    /// An answer for a password changed during the check, or for a window
    /// closed during it, is dropped.
    #[test]
    fn an_answer_for_a_changed_or_closed_window_is_dropped() {
        let mut app = new_app();
        password_window(&mut app, "password-changed.pdf", "first");
        let check = app.update(Message::PdfPassword(PdfPasswordMsg::Ok));
        let _ = app.update(Message::PdfPassword(PdfPasswordMsg::Input(
            "second".to_string(),
        )));
        app.drive_headless_task(check).expect("the check");
        assert!(!wrong(&app), "an answer for another password");

        let check = app.check_pdf_password_with(|_, _, _, _, _| Check::Unlocked);
        let _ = app.cancel_pdf_password();
        app.drive_headless_task(check).expect("the check");
        assert_eq!(app.active_modal, None, "a closed window stays closed");
    }

    /// An answer that unlocks the file closes the window and goes on to
    /// what asked for the password: here the Import PDF window's read runs
    /// again. (The fake answer does not unlock the file, so that read asks
    /// for the password again, in a new window.)
    #[test]
    fn an_unlocking_answer_goes_on() {
        let mut app = new_app();
        password_window(&mut app, "password-unlocks.pdf", "x");
        let check = app.check_pdf_password_with(|_, _, _, _, _| Check::Unlocked);
        app.drive_headless_task(check).expect("the check");
        let reads = app
            .command_line
            .history
            .iter()
            .filter(|entry| entry.text.contains("Reading password-unlocks.pdf"))
            .count();
        assert_eq!(reads, 2, "the window's read runs again");
        assert_eq!(
            app.pdf_password.as_ref().map(|s| s.input.as_str()),
            Some("")
        );
    }
}
