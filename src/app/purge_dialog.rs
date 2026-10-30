//! The Purge dialog: lists what PURGE can remove and runs the command on the
//! ticked items.

use super::{Message, ModalKind, OpenCADStudio};
use crate::ui::window::purge::{PurgeMsg, State, KINDS};
use iced::Task;
use rustc_hash::FxHashSet;

/// Restricts the next PURGE to the dialog's choice.
pub(crate) struct PurgeOnly {
    /// Per kind: the ticked names.
    pub ticked: [FxHashSet<String>; KINDS],
    /// Per kind: everything the dialog listed.
    pub listed: [FxHashSet<String>; KINDS],
    /// Also what removing them frees (it was not listed).
    pub nested: bool,
}

impl PurgeOnly {
    pub(crate) fn allows(&self, kind: usize, name: &str) -> bool {
        self.ticked[kind].contains(name) || (self.nested && !self.listed[kind].contains(name))
    }
}

impl OpenCADStudio {
    fn purge_lists(&self) -> [Vec<String>; KINDS] {
        let i = self.active_tab;
        let open_edits: Vec<codec::Handle> = self.tabs[i]
            .block_edits
            .iter()
            .map(|session| session.br_handle)
            .collect();
        let mut lists =
            super::commands::purge_pass(&self.tabs[i].scene.document, &open_edits, [true; KINDS]);
        for list in &mut lists {
            list.sort_by_key(|name| name.to_lowercase());
        }
        lists
    }

    pub(in crate::app) fn open_purge_dialog(&mut self) {
        let nested = self.purge_dialog.as_ref().is_none_or(|state| state.nested);
        self.purge_dialog = Some(State {
            items: self.purge_lists().map(|names| names.into_iter().map(|n| (n, false)).collect()),
            nested,
        });
        self.active_modal = Some(ModalKind::Purge);
        self.reset_modal_geometry();
    }

    pub(in crate::app) fn on_purge_dialog(&mut self, msg: PurgeMsg) -> Task<Message> {
        let Some(state) = self.purge_dialog.as_mut() else {
            return Task::none();
        };
        let everything = match msg {
            PurgeMsg::Item(kind, index) => {
                if let Some(item) = state.items[kind].get_mut(index) {
                    item.1 = !item.1;
                }
                return Task::none();
            }
            PurgeMsg::Kind(kind) => {
                let all = state.items[kind].iter().all(|(_, ticked)| *ticked);
                for item in &mut state.items[kind] {
                    item.1 = !all;
                }
                return Task::none();
            }
            PurgeMsg::Nested(nested) => {
                state.nested = nested;
                return Task::none();
            }
            PurgeMsg::PurgeChecked => false,
            PurgeMsg::PurgeAll => true,
        };
        let names = |ticked_only: bool| {
            state.items.each_ref().map(|items| {
                items
                    .iter()
                    .filter(|(_, ticked)| *ticked || !ticked_only)
                    .map(|(name, _)| name.clone())
                    .collect::<FxHashSet<_>>()
            })
        };
        self.purge_only = Some(PurgeOnly {
            ticked: names(!everything),
            listed: names(false),
            nested: state.nested,
        });
        let task = self.dispatch_command("PURGE ALL");
        self.purge_only = None;
        // What is left, as the dialog stays open.
        self.open_purge_dialog();
        task
    }
}
