use super::{Message, ModalKind, OpenCADStudio};
use iced::Task;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BCYCLEORDER"]
});

impl OpenCADStudio {
    pub(in crate::app) fn open_block_cycle_order(&mut self) -> Task<Message> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            self.command_line.push_error(
                crate::t!("BCYCLEORDER requires an active Block Editor session.").as_ref(),
            );
            return Task::none();
        };
        match crate::ui::window::block_cycle_order::State::new(
            tab.id,
            session.br_handle,
            &tab.scene.document,
        ) {
            Ok(state) => {
                self.block_cycle_order = Some(state);
                self.active_modal = Some(ModalKind::BlockCycleOrder);
                self.reset_modal_geometry();
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }

    pub(in crate::app) fn apply_block_cycle_order(&mut self) -> Task<Message> {
        if self.active_modal != Some(ModalKind::BlockCycleOrder) {
            return Task::none();
        }
        let Some(state) = self.block_cycle_order.as_mut() else {
            return Task::none();
        };
        let tab = &self.tabs[self.active_tab];
        let current = crate::scene::block_grips::grips(&tab.scene.document, state.definition);
        let valid = tab.id == state.document
            && tab
                .active_block_edit_session()
                .map(|session| session.br_handle)
                == Some(state.definition)
            && current
                .as_ref()
                .is_ok_and(|grips| grips.len() == state.expected.len())
            && state
                .expected
                .iter()
                .all(|(handle, object)| tab.scene.document.objects.get(handle) == Some(object));
        if !valid {
            state.error = Some(
                crate::t!("The grip list or editor session changed; reopen BCYCLEORDER.")
                    .into_owned(),
            );
            return Task::none();
        }
        let rows = state
            .rows
            .iter()
            .map(|row| (row.handle, row.enabled))
            .collect::<Vec<_>>();
        let mut staged = tab.scene.document.clone();
        match crate::scene::block_grips::set_cycle_order(&mut staged, state.definition, &rows) {
            Ok(changed) => {
                if changed {
                    let i = self.active_tab;
                    self.push_undo_snapshot(i, super::document::HistoryLabel::Message("Change insertion grip order"));
                    self.tabs[i].scene.document = staged;
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                    self.finish_pending_history(i);
                    self.refresh_properties();
                }
                self.close_active_modal();
            }
            Err(error) => state.error = Some(crate::t!(error).into_owned()),
        }
        Task::none()
    }
}
