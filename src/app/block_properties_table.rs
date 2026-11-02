use super::{Message, ModalKind, OpenCADStudio};
use crate::scene::block_properties_table as table;
use crate::ui::window::block_properties_table::{Action, State};
use iced::Task;

impl OpenCADStudio {
    pub(in crate::app) fn properties_table_error(&self) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            return Some(
                "Open a block in the Block Editor before editing its Block Properties Table.",
            );
        };
        let definition = session.br_handle;
        if table::columns(&tab.scene.document, definition).is_empty() {
            return Some(
                "Place a Linear or Polar parameter before editing the Block Properties Table.",
            );
        }
        table::table(&tab.scene.document, definition).err()
    }
    pub(in crate::app) fn open_block_properties_table(&mut self) -> Task<Message> {
        if let Some(error) = self.properties_table_error() {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let definition = tab.active_block_edit_session().unwrap().br_handle;
        let original = match table::table(&tab.scene.document, definition) {
            Ok(value) => value,
            Err(error) => {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Task::none();
            }
        };
        let (_, columns, rows) = match table::grid_table(&tab.scene.document, definition) {
            Ok(Some(value)) => value,
            Ok(None) => (codec::Handle::NULL, vec![], vec![]),
            Err(error) => {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Task::none();
            }
        };
        let must_match = original.as_ref().is_some_and(|view| view.must_match);
        self.block_properties_table = Some(State {
            document: tab.id,
            definition,
            epoch: tab.scene.geometry_epoch,
            original,
            must_match,
            columns,
            properties: table::columns(&tab.scene.document, definition),
            selected_property: None,
            rows,
            error: None,
            notice: None,
            confirm_close: false,
        });
        self.active_modal = Some(ModalKind::BlockPropertiesTable);
        Task::none()
    }
    pub(super) fn handle_block_properties_table(&mut self, action: Action) -> Task<Message> {
        if !matches!(action, Action::Apply) {
            if let Some(state) = self.block_properties_table.as_mut() {
                match action {
                    Action::DiscardClose => {
                        // Leaving this way is the same as pressing `Cancel`.
                        self.close_active_modal();
                        return Task::none();
                    }
                    Action::KeepEditing => {
                        // The diagnosis stays on screen; only the offer goes.
                        state.confirm_close = false;
                        return Task::none();
                    }
                    _ => state.edit(action),
                }
            }
            return Task::none();
        }
        let Some(state) = self.block_properties_table.as_ref() else {
            return Task::none();
        };
        let mut cells = Vec::with_capacity(state.rows.len());
        for row in &state.rows {
            let mut parsed = Vec::with_capacity(row.len());
            for value in row {
                match value.trim().parse::<f64>() {
                    Ok(value) if value.is_finite() => parsed.push(value),
                    _ => {
                        let state = self.block_properties_table.as_mut().unwrap();
                        state.error =
                            Some(crate::t!("Each cell must be a finite number.").into_owned());
                        // Refusing the table is not a reason to hold the operator inside
                        // the dialog: offer the way out along with the diagnosis.
                        state.confirm_close = true;
                        return Task::none();
                    }
                }
            }
            cells.push(parsed);
        }
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let stale = tab.id != state.document
            || tab.active_block_edit_session().map(|s| s.br_handle) != Some(state.definition)
            || tab.scene.geometry_epoch != state.epoch
            || table::table(&tab.scene.document, state.definition)
                .ok()
                .flatten()
                .as_ref()
                != state.original.as_ref();
        if stale {
            self.block_properties_table.as_mut().unwrap().error = Some(
                crate::t!("The block changed. Cancel and reopen the Block Properties Table.")
                    .into_owned(),
            );
            return Task::none();
        }
        let unchanged = state.original.as_ref().is_some_and(|view| {
            view.columns == state.columns
                && view.rows.iter().eq(cells.iter())
                && view.must_match == state.must_match
        });
        if unchanged {
            self.block_properties_table = None;
            self.active_modal = None;
            return Task::none();
        }
        let mut working = tab.scene.document.clone();
        if let Err(error) = table::apply(
            &mut working,
            state.definition,
            &state.columns,
            &cells,
            state.must_match,
        ) {
            let state = self.block_properties_table.as_mut().unwrap();
            state.error = Some(crate::t!(error).into_owned());
            state.confirm_close = true;
            return Task::none();
        }
        self.push_undo_snapshot(i, "Block Properties Table");
        self.tabs[i].scene.document = working;
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        self.refresh_properties();
        self.block_properties_table = None;
        self.active_modal = None;
        Task::none()
    }
}
