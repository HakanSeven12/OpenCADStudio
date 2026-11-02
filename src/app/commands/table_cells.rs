//! The right-edge tools of picked table cells: rows and columns before or
//! after the picked ones, removing them, merging and unmerging, locking and
//! editing the first picked cell. Each is one undo step.

use crate::app::{Message, OpenCADStudio};
use codec::entities::table::{CellRange, CellStateFlags, TableCell, TableRow};
use codec::entities::Table;
use iced::Task;

/// A blank cell carrying `cell`'s own style and state, not its content.
fn blank_like(cell: &TableCell) -> TableCell {
    let mut blank = TableCell::new();
    blank.style = cell.style.clone();
    blank.state = cell.state;
    blank
}

/// A row like `source` (height, style, cell styles) without its content.
fn row_like(source: &TableRow) -> TableRow {
    let mut row = source.clone();
    for cell in &mut row.cells {
        *cell = blank_like(cell);
    }
    row
}

impl OpenCADStudio {
    pub(super) fn dispatch_table_cells(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        if !cmd.starts_with("_TCELL") {
            return None;
        }
        let cells = self.tabs[i].scene.table_cells?;
        let (r0, c0, r1, c1) = {
            let Some(codec::EntityType::Table(table)) =
                self.tabs[i].scene.document.get_entity(cells.table)
            else {
                return Some(Task::none());
            };
            crate::entities::table::widen_to_merged(table, cells.range())
        };
        if cmd == "_TCELLEDIT" {
            let Some(codec::EntityType::Table(table)) =
                self.tabs[i].scene.document.get_entity(cells.table)
            else {
                return Some(Task::none());
            };
            let command = crate::modules::annotate::table_cmd::TableCellEditCommand::new(
                cells.table,
                table,
                r0,
                c0,
            );
            use crate::command::CadCommand;
            self.command_line.push_info(&command.prompt());
            self.tabs[i].active_cmd = Some(Box::new(command));
            return Some(self.finish_dispatch(cmd));
        }
        let (label, edit): (&str, Box<dyn Fn(&mut Table) -> Option<(usize, usize, usize, usize)>>) =
            match cmd {
                "_TCELLROWABOVE" => (
                    "TABLE INSERT ROW",
                    Box::new(move |t| {
                        let row = row_like(&t.rows[r0]);
                        t.insert_row(r0);
                        t.rows[r0] = row;
                        Some((r0 + 1, c0, r1 + 1, c1))
                    }),
                ),
                "_TCELLROWBELOW" => (
                    "TABLE INSERT ROW",
                    Box::new(move |t| {
                        let row = row_like(&t.rows[r1]);
                        t.insert_row(r1 + 1);
                        t.rows[r1 + 1] = row;
                        Some((r0, c0, r1, c1))
                    }),
                ),
                "_TCELLDELROWS" => (
                    "TABLE DELETE ROW",
                    Box::new(move |t| {
                        if r1 - r0 + 1 >= t.rows.len() {
                            return None;
                        }
                        for row in (r0..=r1).rev() {
                            t.remove_row(row);
                        }
                        let row = r0.min(t.rows.len() - 1);
                        Some((row, c0, row, c0))
                    }),
                ),
                "_TCELLCOLLEFT" | "_TCELLCOLRIGHT" => {
                    let left = cmd == "_TCELLCOLLEFT";
                    (
                        "TABLE INSERT COLUMN",
                        Box::new(move |t| {
                            let (source, at) = if left { (c0, c0) } else { (c1, c1 + 1) };
                            let mut column = t.columns[source].clone();
                            let cells: Vec<TableCell> =
                                t.rows.iter().map(|row| blank_like(&row.cells[source])).collect();
                            t.insert_column(at, column.width);
                            column.name.clear();
                            t.columns[at] = column;
                            for (row, cell) in t.rows.iter_mut().zip(cells) {
                                row.cells[at] = cell;
                            }
                            Some(if left { (r0, c0 + 1, r1, c1 + 1) } else { (r0, c0, r1, c1) })
                        }),
                    )
                }
                "_TCELLDELCOLS" => (
                    "TABLE DELETE COLUMN",
                    Box::new(move |t| {
                        if c1 - c0 + 1 >= t.columns.len() {
                            return None;
                        }
                        for column in (c0..=c1).rev() {
                            t.remove_column(column);
                        }
                        let column = c0.min(t.columns.len() - 1);
                        Some((r0, column, r0, column))
                    }),
                ),
                "_TCELLMERGE" => (
                    "TABLE MERGE",
                    Box::new(move |t| {
                        if r0 == r1 && c0 == c1 {
                            return None;
                        }
                        t.merge_cells(CellRange::new(r0, c0, r1, c1));
                        Some((r0, c0, r1, c1))
                    }),
                ),
                "_TCELLUNMERGE" => (
                    "TABLE UNMERGE",
                    Box::new(move |t| {
                        let merged: Vec<(usize, usize)> = (r0..=r1)
                            .flat_map(|r| (c0..=c1).map(move |c| (r, c)))
                            .filter(|&(r, c)| {
                                crate::entities::table::widen_to_merged(t, (r, c, r, c)) != (r, c, r, c)
                            })
                            .collect();
                        if merged.is_empty() {
                            return None;
                        }
                        for (row, column) in merged {
                            t.unmerge_cell(row, column);
                        }
                        Some((r0, c0, r1, c1))
                    }),
                ),
                "_TCELLLOCK" => (
                    "TABLE CELL LOCK",
                    Box::new(move |t| {
                        let lock = CellStateFlags::CONTENT_LOCKED | CellStateFlags::FORMAT_LOCKED;
                        // Unlocked if every picked cell is locked, locked otherwise.
                        let all = (r0..=r1).all(|r| {
                            (c0..=c1).all(|c| t.cell(r, c).is_some_and(|cell| cell.state.contains(lock)))
                        });
                        for r in r0..=r1 {
                            for c in c0..=c1 {
                                if let Some(cell) = t.cell_mut(r, c) {
                                    cell.state.set(lock, !all);
                                }
                            }
                        }
                        Some((r0, c0, r1, c1))
                    }),
                ),
                _ => return None,
            };
        let handle = cells.table;
        if self.tabs[i].scene.is_layer_locked(handle) {
            self.command_line.push_info("1 was on a locked layer.");
            return Some(Task::none());
        }
        let mut table = match self.tabs[i].scene.document.get_entity(handle) {
            Some(codec::EntityType::Table(table)) => (**table).clone(),
            _ => return Some(Task::none()),
        };
        let Some(range) = edit(&mut table) else {
            return Some(Task::none());
        };
        self.push_undo_snapshot(i, label);
        if let Some(codec::EntityType::Table(target)) =
            self.tabs[i].scene.document.get_entity_mut(handle)
        {
            **target = table;
            target.block_record_handle = None;
        }
        self.tabs[i]
            .scene
            .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
        self.tabs[i].dirty = true;
        self.tabs[i].scene.select_table_cells(crate::scene::TableCellSelection {
            table: handle,
            anchor: (range.0, range.1),
            end: (range.2, range.3),
        });
        self.refresh_properties();
        Some(self.finish_dispatch(cmd))
    }
}

impl OpenCADStudio {
    /// Tab / Shift+Tab in a cell's editor: the typed text goes into the cell
    /// and the editor moves on to the next (previous) cell, row by row,
    /// skipping the covered parts of merged cells. Tab in the last cell adds a
    /// row like the last one (its own undo step).
    pub(in crate::app) fn table_cell_tab(&mut self, forward: bool) -> Option<Task<Message>> {
        let i = self.active_tab;
        let (handle, row, column) = self.tabs[i].active_cmd.as_ref()?.table_cell()?;
        let text = std::mem::take(&mut self.command_line.input);
        let committed = if text.is_empty() {
            self.tabs[i].active_cmd = None;
            Task::none()
        } else {
            self.feed_command(crate::command::StepInput::Text(text))
        };
        self.tabs[i].active_cmd = None;
        let Some(codec::EntityType::Table(table)) = self.tabs[i].scene.document.get_entity(handle)
        else {
            return Some(committed);
        };
        let columns = table.column_count();
        let count = table.row_count() * columns;
        let owner = |index: usize| {
            let (r, c) = (index / columns, index % columns);
            let (r0, c0, _, _) = crate::entities::table::widen_to_merged(table, (r, c, r, c));
            (r0, c0) == (r, c)
        };
        let start = row * columns + column;
        let mut next = None;
        if forward {
            next = (start + 1..count).find(|&index| owner(index));
        } else if start > 0 {
            next = (0..start).rev().find(|&index| owner(index));
        }
        let (row, column) = match next {
            Some(index) => (index / columns, index % columns),
            None if forward => {
                // Past the last cell: a new row like the last one.
                let mut table = (**table).clone();
                let last = table.rows.len() - 1;
                let source = row_like(&table.rows[last]);
                table.rows.push(source);
                table.block_record_handle = None;
                self.push_undo_snapshot(i, "TABLE INSERT ROW");
                if let Some(codec::EntityType::Table(target)) =
                    self.tabs[i].scene.document.get_entity_mut(handle)
                {
                    **target = table;
                }
                self.tabs[i]
                    .scene
                    .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                self.tabs[i].dirty = true;
                (last + 1, 0)
            }
            None => (row, column),
        };
        let Some(codec::EntityType::Table(table)) = self.tabs[i].scene.document.get_entity(handle)
        else {
            return Some(committed);
        };
        let command =
            crate::modules::annotate::table_cmd::TableCellEditCommand::new(handle, table, row, column);
        use crate::command::CadCommand;
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Some(Task::batch([committed, self.focus_cmd_input()]))
    }
}
