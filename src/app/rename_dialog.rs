//! The RENAME dialog's state and messages; the renaming itself is
//! `rename_named_object`, shared with the command-line form.

use super::{Message, ModalKind, OpenCADStudio};
use crate::ui::window::rename::{RenameMsg, State, KINDS};
use codec::TableEntry;
use iced::Task;

/// The renamable items of `kind` in `doc`: no standard, anonymous or
/// externally referenced names.
fn items(doc: &codec::Document, kind: &str) -> Vec<String> {
    let names: Vec<String> = match kind {
        "BLOCK" => doc
            .block_records
            .iter()
            .filter(|br| !br.is_standard() && !br.is_layout() && !br.name.starts_with('*'))
            .map(|br| br.name.clone())
            .collect(),
        "DIMSTYLE" => doc.dim_styles.iter().map(|s| s.name.clone()).collect(),
        "LAYER" => doc
            .layers
            .iter()
            .map(|l| l.name.clone())
            .filter(|name| name != "0" && !name.eq_ignore_ascii_case("DEFPOINTS"))
            .collect(),
        "LINETYPE" => doc
            .line_types
            .iter()
            .map(|l| l.name.clone())
            .filter(|name| {
                !["BYLAYER", "BYBLOCK", "CONTINUOUS"]
                    .iter()
                    .any(|fixed| name.eq_ignore_ascii_case(fixed))
            })
            .collect(),
        "STYLE" => doc
            .text_styles
            .iter()
            .map(|s| s.name.clone())
            .filter(|name| !name.is_empty())
            .collect(),
        "UCS" => doc.ucss.iter().map(|u| u.name.clone()).collect(),
        _ => doc.views.iter().map(|v| v.name.clone()).collect(),
    };
    let mut names: Vec<String> = names.into_iter().filter(|name| !name.contains('|')).collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}

impl OpenCADStudio {
    pub(in crate::app) fn open_rename_dialog(&mut self) {
        let doc = &self.tabs[self.active_tab].scene.document;
        let kind = KINDS.iter().position(|kind| *kind == "LAYER").unwrap_or(0);
        self.rename_dialog = Some(State {
            kind,
            items: items(doc, KINDS[kind]),
            item: None,
            new_name: String::new(),
        });
        self.active_modal = Some(ModalKind::Rename);
        self.reset_modal_geometry();
    }

    pub(in crate::app) fn on_rename_dialog(&mut self, msg: RenameMsg) -> Task<Message> {
        let i = self.active_tab;
        let Some(state) = self.rename_dialog.as_mut() else {
            return Task::none();
        };
        match msg {
            RenameMsg::Kind(kind) => {
                state.kind = kind;
                state.items = items(&self.tabs[i].scene.document, KINDS[kind]);
                state.item = None;
                state.new_name.clear();
            }
            RenameMsg::Item(name) => state.item = Some(name),
            RenameMsg::NewName(name) => state.new_name = name,
            RenameMsg::Rename | RenameMsg::Ok => {
                let kind = KINDS[state.kind];
                let new_name = state.new_name.trim().to_string();
                if let Some(old_name) = state.item.clone().filter(|_| !new_name.is_empty()) {
                    if self.rename_named_object(i, kind, &old_name, &new_name) {
                        if let Some(state) = self.rename_dialog.as_mut() {
                            state.items = items(&self.tabs[i].scene.document, kind);
                            state.item = Some(new_name);
                            state.new_name.clear();
                        }
                    } else {
                        // Keep the dialog open on the refusal.
                        return Task::none();
                    }
                }
                if matches!(msg, RenameMsg::Ok) {
                    self.close_active_modal();
                }
            }
        }
        Task::none()
    }
}

