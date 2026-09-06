use super::{ModalKind, OpenCADStudio};

impl OpenCADStudio {
    pub(super) fn queue_startup_prompts(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.default_assoc_prompted {
            self.pending_startup_modals
                .push_back(ModalKind::AssocPrompt);
        }
        if self.donation_prompt_version != env!("OCS_APP_VERSION") {
            self.pending_startup_modals
                .push_back(ModalKind::DonationPrompt);
        }
        self.show_next_startup_modal();
    }

    pub(super) fn show_next_startup_modal(&mut self) {
        if self.active_modal.is_none() && self.opening.is_none() && self.pending_opens.is_empty() {
            if let Some(&kind) = self.pending_startup_modals.front() {
                self.active_modal = Some(kind);
                self.reset_modal_geometry();
            }
        }
    }

    pub(super) fn mark_startup_modal_shown(&mut self) {
        if self.active_modal.is_some()
            && self.active_modal.as_ref() == self.pending_startup_modals.front()
        {
            self.pending_startup_modals.pop_front();
        }
        if self.active_modal == Some(ModalKind::DonationPrompt)
            && self.donation_prompt_version != env!("OCS_APP_VERSION")
        {
            self.donation_prompt_version = env!("OCS_APP_VERSION").to_string();
            self.save_config();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{config::AppConfig, Message};

    #[test]
    fn donation_prompt_persists_once_per_version() {
        let mut app = OpenCADStudio::new_for_test();
        // Older settings have no donation version; headless construction never prompts.
        app.apply_config(serde_json::from_str::<AppConfig>("{}").unwrap());
        assert!(app.active_modal.is_none());
        assert!(app.pending_startup_modals.is_empty());
        app.default_assoc_prompted = true;
        app.queue_startup_prompts();
        assert_eq!(app.active_modal, Some(ModalKind::DonationPrompt));
        assert!(app.donation_prompt_version.is_empty());

        let _ = app.update(Message::ModalContentResized(iced::Size::new(540.0, 280.0)));
        let saved = serde_json::to_string(&app.current_config()).unwrap();
        let _ = app.update(Message::CloseModal);
        assert!(app.active_modal.is_none());

        let mut restarted = OpenCADStudio::new_for_test();
        restarted.apply_config(serde_json::from_str(&saved).unwrap());
        restarted.queue_startup_prompts();
        assert_eq!(restarted.donation_prompt_version, env!("OCS_APP_VERSION"));
        assert!(restarted.active_modal.is_none());

        restarted.donation_prompt_version = "previous-release".to_string();
        restarted.queue_startup_prompts();
        assert_eq!(restarted.active_modal, Some(ModalKind::DonationPrompt));
        let _ = restarted.update(Message::CommandEscape);
        assert!(restarted.active_modal.is_none());
        assert_eq!(restarted.donation_prompt_version, env!("OCS_APP_VERSION"));
        let _ = restarted.update(Message::Noop);
        assert!(restarted.active_modal.is_none());
    }

    #[test]
    fn startup_dialogs_wait_their_turn_and_preserve_unseen_prompts() {
        let mut app = OpenCADStudio::new_for_test();
        app.apply_config(AppConfig::default());
        app.queue_startup_prompts();
        assert_eq!(app.active_modal, Some(ModalKind::AssocPrompt));
        let _ = app.update(Message::UpdateCheckResult(Some(
            crate::io::update_check::UpdateInfo {
                version: "next-release".to_string(),
                body: String::new(),
            },
        )));
        assert_eq!(app.active_modal, Some(ModalKind::AssocPrompt));
        let _ = app.update(Message::AssocPromptNo);
        assert_eq!(app.active_modal, Some(ModalKind::DonationPrompt));

        // A file-recovery dialog may interrupt startup before the first frame.
        app.active_modal = Some(ModalKind::Recovery);
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.active_modal, Some(ModalKind::DonationPrompt));
        assert!(app.donation_prompt_version.is_empty());
        let _ = app.update(Message::ModalContentResized(iced::Size::new(540.0, 280.0)));
        assert_eq!(app.donation_prompt_version, env!("OCS_APP_VERSION"));
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.active_modal, Some(ModalKind::UpdateNotice));
        let _ = app.update(Message::UpdateNoticeClose);
        assert!(app.active_modal.is_none());
        assert!(app.pending_startup_modals.is_empty());
    }
}
