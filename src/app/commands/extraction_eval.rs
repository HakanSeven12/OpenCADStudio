//! DXEVAL: opening, saving, plotting or publishing a drawing compares its
//! extraction tables with the data they were made from. An outdated table is
//! either updated at once or offered in a window, by the event's bits in
//! DXEVAL. The open check runs as the drawing opens, the plot check as the
//! PLOT command starts (not -PLOT or a plot through automation), the publish
//! check as publishing starts. The archive bits (16, 256) are kept in the
//! value but packaging a drawing does not compare its tables.

use crate::app::annotation_data::{self, OutdatedExtraction};
use crate::app::{ExtractionPrompt, Message, ModalKind, OpenCADStudio};
use crate::ui::window::extraction_outdated::OutdatedChoice;
use iced::Task;

/// DXEVAL event bits. Save, plot and publish shifted four bits up (32, 64,
/// 128) update without asking; an open only asks.
pub(crate) const EVENT_OPEN: i32 = 1;
pub(crate) const EVENT_SAVE: i32 = 2;
pub(crate) const EVENT_PLOT: i32 = 4;
pub(crate) const EVENT_PUBLISH: i32 = 8;

impl OpenCADStudio {
    /// Writes `rows` into each linked table (from `first_linked_row` on) and
    /// stamps its link's update time, as one undo step; the tables updated.
    pub(crate) fn apply_link_updates(
        &mut self,
        i: usize,
        updates: Vec<(codec::Handle, codec::Handle, Vec<Vec<String>>, usize)>,
    ) -> usize {
        if updates.is_empty() {
            return 0;
        }
        self.push_undo_snapshot(i, "DATALINKUPDATE");
        let mut changed = Vec::new();
        for (table_handle, link_handle, rows, first_linked_row) in updates {
            let document = &mut self.tabs[i].scene.document;
            let Some(codec::EntityType::Table(table)) = document.get_entity_mut(table_handle)
            else {
                continue;
            };
            crate::app::commands::display::apply_link_rows(
                table,
                link_handle,
                &rows,
                first_linked_row,
            );
            // The stored picture no longer shows the cells: draw from them.
            table.block_record_handle = None;
            annotation_data::stamp_data_link_handle(document, link_handle);
            changed.push(table_handle);
        }
        if !changed.is_empty() {
            self.invalidate_property_targets(i, &changed);
            self.tabs[i].dirty = true;
            self.refresh_properties();
        }
        changed.len()
    }

    /// The DXEVAL check of `event` on drawing `i`. `Some` when the event
    /// waits for the outdated table window; `resume` carries it on once the
    /// window is answered.
    pub(crate) fn evaluate_extraction_tables(
        &mut self,
        i: usize,
        event: i32,
        resume: Option<Message>,
    ) -> Option<Task<Message>> {
        if std::mem::take(&mut self.extraction_checked) || i >= self.tabs.len() {
            return None;
        }
        let dxeval = annotation_data::dxeval(&self.tabs[i].scene.document);
        let automatic = event != EVENT_OPEN && dxeval & (event << 4) != 0;
        if !automatic && dxeval & event == 0 {
            return None;
        }
        let outdated = annotation_data::outdated_extraction_tables(
            &self.tabs[i].scene.document,
            self.tabs[i].current_path.as_deref(),
        );
        if outdated.is_empty() {
            return None;
        }
        if automatic {
            self.update_outdated(i, outdated);
            return None;
        }
        self.extraction_outdated = Some(ExtractionPrompt {
            count: outdated.len(),
            tables: outdated,
            resume,
        });
        self.active_modal = Some(ModalKind::ExtractionOutdated);
        self.reset_modal_geometry();
        Some(Task::none())
    }

    fn update_outdated(&mut self, i: usize, tables: Vec<OutdatedExtraction>) {
        let updates = tables
            .into_iter()
            .map(|table| (table.table, table.link, table.rows, 1))
            .collect();
        self.apply_link_updates(i, updates);
    }

    /// One answer of the outdated table window; the event goes on when no
    /// table is left to answer for.
    pub(crate) fn on_extraction_outdated(&mut self, choice: OutdatedChoice) -> Task<Message> {
        let Some(mut prompt) = self.extraction_outdated.take() else {
            return Task::none();
        };
        let i = self.active_tab;
        match choice {
            OutdatedChoice::Update => {
                if !prompt.tables.is_empty() {
                    let first = prompt.tables.remove(0);
                    self.update_outdated(i, vec![first]);
                }
            }
            OutdatedChoice::UpdateAll => {
                let tables = std::mem::take(&mut prompt.tables);
                self.update_outdated(i, tables);
            }
            OutdatedChoice::Skip => {
                if !prompt.tables.is_empty() {
                    prompt.tables.remove(0);
                }
            }
            OutdatedChoice::SkipAll => prompt.tables.clear(),
        }
        if !prompt.tables.is_empty() {
            self.extraction_outdated = Some(prompt);
            return Task::none();
        }
        if self.active_modal == Some(ModalKind::ExtractionOutdated) {
            self.active_modal = None;
            self.reset_modal_geometry();
        }
        match prompt.resume {
            Some(message) => {
                self.extraction_checked = true;
                Task::done(message)
            }
            None => Task::none(),
        }
    }

    /// `DXEVAL [value]` and `SETVAR DXEVAL value`: 0–511, kept in the
    /// drawing.
    pub(super) fn dispatch_dxeval(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !name.trim_start_matches(['_', '.']).eq_ignore_ascii_case("DXEVAL") {
            return None;
        }
        let current = annotation_data::dxeval(&self.tabs[i].scene.document);
        let value = value.trim();
        match value.parse::<i32>() {
            Ok(n) if (0..=511).contains(&n) => {
                if n != current {
                    crate::io::set_drawing_variable(
                        &mut self.tabs[i].scene.document,
                        "DXEVAL",
                        &n.to_string(),
                    );
                    self.tabs[i].dirty = true;
                }
            }
            _ => {
                if !value.is_empty() {
                    let (min, max) = (0, 511);
                    self.command_line.push_error(&crate::tf!(
                        "Requires an integer between {min} and {max}."
                    ));
                }
                self.command_line
                    .push_output(&format!("Enter new value for DXEVAL <{current}>:"));
                self.pending_setvar = Some("DXEVAL".into());
            }
        }
        Some(Task::none())
    }
}
