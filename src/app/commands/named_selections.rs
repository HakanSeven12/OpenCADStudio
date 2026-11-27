//! NAMEDSEL: selection sets saved by name in the drawing. The typed form
//! `NAMEDSEL <SAVE|RECALL|RENAME|DELETE|?> ...` makes each change; the
//! interactive command collects its input and dispatches that form.

use super::*;
use crate::scene::{NamedSelectionRecall, Scene};
use document::Handle;

/// Why a set was not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum NamedSelectionSaveError {
    InvalidName,
    NothingToSave,
}

/// A name a set may take: a symbol name (as a block name) of at most 255
/// characters, with no spaces, because the typed form splits on them.
pub(in crate::app) fn valid_named_selection_name(name: &str) -> bool {
    name.chars().count() <= 255
        && !name.contains(char::is_whitespace)
        && crate::scene::valid_block_name(name)
}

impl OpenCADStudio {
    /// Save a set in one undo step. Returns whether a set was replaced and
    /// how many members the set holds.
    pub(in crate::app) fn save_named_selection_step(
        &mut self,
        i: usize,
        name: &str,
        handles: &[Handle],
    ) -> Result<(bool, usize), NamedSelectionSaveError> {
        if !valid_named_selection_name(name) {
            return Err(NamedSelectionSaveError::InvalidName);
        }
        let scene = &self.tabs[i].scene;
        if !handles
            .iter()
            .any(|handle| scene.document.get_entity(*handle).is_some())
        {
            return Err(NamedSelectionSaveError::NothingToSave);
        }
        let pending = self.begin_undo(i, "NAMEDSEL", 0, true);
        let replaced = self.tabs[i].scene.save_named_selection(name, handles);
        self.tabs[i].dirty = true;
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        let count = self.tabs[i]
            .scene
            .named_selection(name)
            .map_or(0, |(_, members)| members.len());
        Ok((replaced, count))
    }

    /// Run a rename or delete of a set in one undo step.
    fn named_selection_edit_step(&mut self, i: usize, edit: impl FnOnce(&mut Scene) -> bool) {
        let pending = self.begin_undo(i, "NAMEDSEL", 0, true);
        if edit(&mut self.tabs[i].scene) {
            self.tabs[i].dirty = true;
        }
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
    }

    /// Say how many members a recall found, and how many it left out.
    fn report_named_selection_recall(&mut self, recall: &NamedSelectionRecall) {
        let found = recall.selectable.len();
        self.command_line
            .push_info(crate::tf!("{found} found").as_ref());
        self.report_named_selection_left_out(recall);
    }

    /// Say how many members a recall left out, and why.
    pub(in crate::app) fn report_named_selection_left_out(
        &mut self,
        recall: &NamedSelectionRecall,
    ) {
        if recall.erased > 0 {
            let count = recall.erased;
            self.command_line
                .push_info(crate::tf!("{count} erased objects not selected.").as_ref());
        }
        if recall.other_space > 0 {
            let count = recall.other_space;
            self.command_line
                .push_info(crate::tf!("{count} objects in other spaces not selected.").as_ref());
        }
        if recall.unpickable > 0 {
            let count = recall.unpickable;
            self.command_line
                .push_info(crate::tf!("{count} hidden or locked objects not selected.").as_ref());
        }
    }

    pub(super) fn dispatch_named_selections(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        let mut words = cmd.split_whitespace();
        if !words.next()?.eq_ignore_ascii_case("NAMEDSEL") {
            return None;
        }
        let words: Vec<&str> = words.collect();
        let Some(sub) = words.first().map(|word| word.to_ascii_uppercase()) else {
            // NAMEDSEL on its own: Save takes the objects selected now.
            use crate::modules::draw::groups::namedsel::NamedSelCommand;
            let scene = &self.tabs[i].scene;
            let names = scene
                .named_selections()
                .into_iter()
                .map(|(name, _)| name)
                .collect();
            let command = NamedSelCommand::new(scene.selected_handles_in_order(), names);
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Some(Task::none());
        };
        let arg = |index: usize| words.get(index).copied().unwrap_or("");
        match sub.as_str() {
            "SAVE" => {
                let name = arg(1);
                let handles: Vec<Handle> = words
                    .iter()
                    .skip(2)
                    .filter_map(|word| u64::from_str_radix(word, 16).ok())
                    .map(Handle::new)
                    .collect();
                match self.save_named_selection_step(i, name, &handles) {
                    Ok((replaced, count)) => {
                        let text = if replaced {
                            crate::tf!("Selection set \"{name}\" replaced ({count} objects).")
                        } else {
                            crate::tf!("Selection set \"{name}\" saved ({count} objects).")
                        };
                        self.command_line.push_output(text.as_ref());
                    }
                    Err(NamedSelectionSaveError::InvalidName) => self
                        .command_line
                        .push_error(crate::tf!("NAMEDSEL: '{name}' is not a valid name.").as_ref()),
                    Err(NamedSelectionSaveError::NothingToSave) => self
                        .command_line
                        .push_error(crate::t!("NAMEDSEL: nothing to save.").as_ref()),
                }
            }
            "RECALL" => {
                let name = arg(1);
                let Some(recall) = self.tabs[i].scene.named_selection_recall(name) else {
                    self.command_line.push_error(
                        crate::tf!("NAMEDSEL: no selection set named '{name}'.").as_ref(),
                    );
                    return Some(Task::none());
                };
                for &handle in &recall.selectable {
                    self.tabs[i].scene.select_entity(handle, false);
                }
                self.refresh_properties();
                self.refresh_selected_grips();
                self.report_named_selection_recall(&recall);
            }
            "RENAME" => {
                let (old, new) = (arg(1), arg(2));
                let scene = &self.tabs[i].scene;
                if scene.named_selection(old).is_none() {
                    let name = old;
                    self.command_line.push_error(
                        crate::tf!("NAMEDSEL: no selection set named '{name}'.").as_ref(),
                    );
                } else if !valid_named_selection_name(new) {
                    let name = new;
                    self.command_line
                        .push_error(crate::tf!("NAMEDSEL: '{name}' is not a valid name.").as_ref());
                } else if scene
                    .named_selection(new)
                    .is_some_and(|(found, _)| !found.eq_ignore_ascii_case(old))
                {
                    let name = new;
                    self.command_line.push_error(
                        crate::tf!("NAMEDSEL: a selection set named '{name}' already exists.")
                            .as_ref(),
                    );
                } else {
                    self.named_selection_edit_step(i, |scene| {
                        scene.rename_named_selection(old, new)
                    });
                    self.command_line.push_output(
                        crate::tf!("NAMEDSEL: renamed '{old}' to '{new}'.").as_ref(),
                    );
                }
            }
            "DELETE" => {
                let name = arg(1);
                if self.tabs[i].scene.named_selection(name).is_none() {
                    self.command_line.push_error(
                        crate::tf!("NAMEDSEL: no selection set named '{name}'.").as_ref(),
                    );
                } else {
                    self.named_selection_edit_step(i, |scene| {
                        scene.delete_named_selection(name)
                    });
                    self.command_line
                        .push_output(crate::tf!("NAMEDSEL: deleted '{name}'.").as_ref());
                }
            }
            "?" | "LIST" => {
                let sets = self.tabs[i].scene.named_selections();
                if sets.is_empty() {
                    self.command_line.push_info(
                        crate::t!("NAMEDSEL: no selection sets in this drawing.").as_ref(),
                    );
                } else {
                    self.command_line
                        .push_output(crate::t!("Selection sets:").as_ref());
                    for (name, members) in sets {
                        let count = members.len();
                        self.command_line
                            .push_output(crate::tf!("{name} ({count} objects)").as_ref());
                    }
                }
            }
            _ => self.command_line.push_error(
                crate::t!(
                    "Usage: NAMEDSEL [SAVE <name> <handle>... | RECALL <name> | RENAME <old> <new> | DELETE <name> | ?]"
                )
                .as_ref(),
            ),
        }
        Some(Task::none())
    }
}
