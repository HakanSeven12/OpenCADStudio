//! Native rendering + host-local EDITING of plugin table surfaces
//! (api v29, S07).
//!
//! Ownership split (the scene/widget tier law): the plugin owns the
//! DATA; the host owns the editing session — draft cells are HOST-LOCAL
//! transient state, and a committed edit is a best-effort
//! `SurfaceEvent::CellEdited` notification. The plugin stays the data
//! authority: it canonicalizes by pushing its own `TablePatch` through
//! the normal wire path; until then the host shows its local draft.
//!
//! Rendering is native widgets only — a scrollable grid of header +
//! row cells, edits through text inputs bound to the draft state. The
//! typed-commit rule: an edit takes the TYPE of the cell it replaces
//! (Number edits parse, Bool edits parse true/false, Text is free); a
//! failed parse keeps the draft uncommitted.

use crate::app::Message;
use crate::app::panel_events::PanelEventRouter;
use iced::widget::{column, container, row, scrollable, text, text_input};
use iced::{Element, Length};
use ocs_plugin_api::scene::StableId;
use ocs_plugin_api::tables::{CellValue, TableSurface};
use std::collections::{BTreeMap, HashSet};

/// Hard cap on render recursion (tables are flat; this bounds the row
/// walk defensively).
const MAX_RENDER_ROWS: usize = 10_000;

/// Host-local editing session for one panel's table surface: draft cell
/// texts keyed by (row id, column id), and the selected cell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableUiState {
    /// In-progress edits: the TEXT the user typed, keyed by cell.
    pub drafts: BTreeMap<(StableId, StableId), String>,
    pub selected: Option<(StableId, StableId)>,
}

impl TableUiState {
    /// The value a cell should DISPLAY: the draft while editing, else
    /// the stored cell's text.
    pub fn display_text(
        &self,
        row: StableId,
        column: StableId,
        stored: Option<&CellValue>,
    ) -> String {
        if let Some(draft) = self.drafts.get(&(row, column)) {
            return draft.clone();
        }
        match stored {
            Some(CellValue::Text(t)) => t.clone(),
            Some(CellValue::Number(n)) => n.to_string(),
            Some(CellValue::Bool(b)) => b.to_string(),
            None => String::new(),
        }
    }

    /// Record a keystroke: the draft updates (or is created); the stored
    /// table is untouched.
    pub fn set_draft(&mut self, row: StableId, column: StableId, text: String) {
        self.drafts.insert((row, column), text);
    }

    /// Commit a draft under the typed-commit rule: `Ok(value)` carries
    /// the typed cell (the existing cell's type governs the parse);
    /// `Err(())` keeps the draft (a failed Number/Bool parse is not a
    /// commit). The draft clears ONLY on success — the user's text
    /// survives until the plugin canonicalizes.
    pub fn commit_draft(
        &mut self,
        row: StableId,
        column: StableId,
        stored: Option<&CellValue>,
    ) -> Result<CellValue, ()> {
        let draft = self.drafts.remove(&(row, column)).ok_or(())?;
        let text = draft.trim().to_owned();
        match stored {
            Some(CellValue::Number(_)) => text
                .parse::<f64>()
                .map(|n| CellValue::Number(n))
                .map_err(|_| {
                    self.drafts.insert((row, column), draft);
                }),
            Some(CellValue::Bool(_)) => match text.as_str() {
                "true" => Ok(CellValue::Bool(true)),
                "false" => Ok(CellValue::Bool(false)),
                _ => {
                    self.drafts.insert((row, column), draft);
                    Err(())
                }
            },
            Some(CellValue::Text(_)) | None => Ok(CellValue::Text(text)),
        }
    }

    /// Select a cell (host-local highlight).
    pub fn select(&mut self, cell: (StableId, StableId)) {
        self.selected = Some(cell);
    }
}

/// Render one table surface with the host's native widgets. `drafts_out`
/// is unused by the view itself; edits flow as Messages and the dispatch
/// mutates the caller's [`TableUiState`].
pub(crate) fn table_view<'a>(
    owner: &'a str,
    panel: &'a str,
    surface: &TableSurface,
    ui: Option<&TableUiState>,
) -> Element<'a, Message> {
    let default_ui = TableUiState::default();
    let ui = ui.unwrap_or(&default_ui);
    // Header row.
    let mut header = row![].spacing(2);
    for col in &surface.columns {
        header = header.push(
            container(text(col.label.clone()).size(12))
                .width(Length::Fill)
                .padding(iced::Padding {
                    top: 2.0,
                    right: 2.0,
                    bottom: 2.0,
                    left: 2.0,
                }),
        );
    }
    let mut grid = column![header].spacing(2);
    let mut count = 0usize;
    for row_id in &surface.order {
        if count >= MAX_RENDER_ROWS {
            break;
        }
        count += 1;
        let Some(cells) = surface.rows.get(row_id) else {
            continue;
        };
        let mut row_widget = row![].spacing(2);
        for (index, col) in surface.columns.iter().enumerate() {
            let stored = cells.get(index);
            let display = ui.display_text(*row_id, col.id, stored);
            let (owner, panel) = (owner.to_owned(), panel.to_owned());
            let (r, c) = (*row_id, col.id);
            let (owner_in, panel_in) = (owner.clone(), panel.clone());
            let (owner_sub, panel_sub) = (owner.clone(), panel.clone());
            let cell = text_input("", display.as_str())
                .size(12)
                .width(Length::Fill)
                .on_input(move |text| Message::PanelTableCellDraft {
                    owner: owner_in.clone(),
                    panel: panel_in.clone(),
                    row: r,
                    column: c,
                    text,
                })
                .on_submit(Message::PanelTableCellCommitted {
                    owner: owner_sub,
                    panel: panel_sub,
                    row: r,
                    column: c,
                });
            row_widget = row_widget.push(container(cell).width(Length::Fill));
        }
        grid = grid.push(row_widget);
    }
    scrollable(container(grid.width(Length::Fill))).into()
}

/// The typed-commit helper shared by the dispatch: resolves the typed
/// value for a cell against the stored surface, or `None` when the draft
/// must stay (failed parse) or the cell/column is unknown.
pub(crate) fn typed_commit(
    surface: &TableSurface,
    row: StableId,
    column: StableId,
    draft: &str,
) -> Option<CellValue> {
    let cells = surface.rows.get(&row)?;
    let index = surface.columns.iter().position(|c| c.id == column)?;
    let stored = cells.get(index);
    let trimmed = draft.trim().to_owned();
    match stored {
        Some(CellValue::Number(_)) => trimmed.parse::<f64>().ok().map(CellValue::Number),
        Some(CellValue::Bool(_)) => match trimmed.as_str() {
            "true" => Some(CellValue::Bool(true)),
            "false" => Some(CellValue::Bool(false)),
            _ => None,
        },
        _ => Some(CellValue::Text(trimmed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::tables::{TableColumn, TableRevision};

    fn table() -> TableSurface {
        let mut t = TableSurface::new(
            TableRevision(1),
            vec![
                TableColumn {
                    id: StableId(1),
                    label: "name".to_owned(),
                },
                TableColumn {
                    id: StableId(2),
                    label: "len".to_owned(),
                },
            ],
        );
        t.apply_patch(&ocs_plugin_api::tables::TablePatch {
            base_revision: TableRevision(1),
            ops: vec![ocs_plugin_api::tables::TableOp::InsertRow {
                after: None,
                row: StableId(10),
                cells: vec![CellValue::Text("alpha".to_owned()), CellValue::Number(1.0)],
            }],
        })
        .expect("seed");
        t
    }

    #[test]
    fn drafts_shadow_the_stored_value_until_committed() {
        let mut ui = TableUiState::default();
        let t = table();
        ui.set_draft(StableId(10), StableId(2), "99".to_owned());
        assert_eq!(
            ui.display_text(StableId(10), StableId(2), Some(&CellValue::Number(1.0))),
            "99",
            "the draft shadows the stored cell"
        );
        ui.set_draft(StableId(10), StableId(1), "beta".to_owned());
        assert_eq!(
            ui.display_text(
                StableId(10),
                StableId(1),
                Some(&CellValue::Text("alpha".to_owned()))
            ),
            "beta"
        );
        let _ = t;
    }

    #[test]
    fn typed_commit_takes_the_existing_cell_type() {
        let mut ui = TableUiState::default();
        let t = table();
        ui.set_draft(StableId(10), StableId(2), "42".to_owned());
        assert_eq!(
            typed_commit(&t, StableId(10), StableId(2), "42"),
            Some(CellValue::Number(42.0)),
            "a Number cell parses the edit"
        );
        ui.set_draft(StableId(10), StableId(1), "beta".to_owned());
        assert_eq!(
            typed_commit(&t, StableId(10), StableId(1), "beta"),
            Some(CellValue::Text("beta".to_owned()))
        );
    }

    #[test]
    fn failed_parse_keeps_the_draft_and_commits_nothing() {
        let mut ui = TableUiState::default();
        let t = table();
        ui.set_draft(StableId(10), StableId(2), "not-a-number".to_owned());
        assert_eq!(
            typed_commit(&t, StableId(10), StableId(2), "not-a-number"),
            None
        );
        assert_eq!(
            ui.drafts
                .get(&(StableId(10), StableId(2)))
                .map(String::as_str),
            Some("not-a-number"),
            "the user's text survives a failed parse"
        );
        // A later valid draft commits.
        ui.set_draft(StableId(10), StableId(2), "7".to_owned());
        assert_eq!(
            typed_commit(&t, StableId(10), StableId(2), "7"),
            Some(CellValue::Number(7.0))
        );
    }
}
