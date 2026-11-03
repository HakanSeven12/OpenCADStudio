//! The right-edge tools of picked table cells: rows and columns before or
//! after the picked ones, removing them, merging and unmerging, locking,
//! matching, alignment, borders, data format, blocks, fields, formulas,
//! contents, linked data and editing the first picked cell. Each edit is one
//! undo step.

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
        if let Some(task) = self.dispatch_table_cell_tools(cmd, i, (r0, c0)) {
            let finish = self.finish_dispatch(cmd);
            return Some(Task::batch([task, finish]));
        }
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
                            // Rows read from a file can be short of cells.
                            let cells: Vec<TableCell> = t
                                .rows
                                .iter()
                                .map(|row| row.cells.get(source).map(blank_like).unwrap_or_default())
                                .collect();
                            t.insert_column(at, column.width);
                            column.name.clear();
                            t.columns[at] = column;
                            for (row, cell) in t.rows.iter_mut().zip(cells) {
                                if let Some(slot) = row.cells.get_mut(at) {
                                    *slot = cell;
                                }
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
    /// The cells a cell property edit goes into: every owner cell of the
    /// picked range, else the panel's current cell.
    pub(in crate::app) fn picked_cell_positions(&self, i: usize) -> Vec<(usize, usize)> {
        let scene = &self.tabs[i].scene;
        if let Some(cells) = scene.table_cells {
            if let Some(codec::EntityType::Table(table)) = scene.document.get_entity(cells.table) {
                let (r0, c0, r1, c1) =
                    crate::entities::table::widen_to_merged(table, cells.range());
                return (r0..=r1)
                    .flat_map(|row| (c0..=c1).map(move |column| (row, column)))
                    .filter(|&(row, column)| {
                        let (o, oc, _, _) = crate::entities::table::widen_to_merged(
                            table,
                            (row, column, row, column),
                        );
                        (o, oc) == (row, column)
                    })
                    .collect();
            }
        }
        let index = self.tabs[i].properties.prop_vertex;
        let columns = scene
            .selected_entities()
            .first()
            .and_then(|(_, entity)| match entity {
                codec::EntityType::Table(table) => Some(table.column_count()),
                _ => None,
            })
            .unwrap_or(0);
        if columns == 0 {
            return Vec::new();
        }
        vec![(index / columns, index % columns)]
    }

    /// Tab / Shift+Tab in a cell's editor: the typed text goes into the cell
    /// and the editor moves on to the next (previous) cell, row by row,
    /// skipping the covered parts of merged cells. Tab in the last cell adds a
    /// row like the last one (its own undo step).
    pub(in crate::app) fn table_cell_tab(&mut self, forward: bool) -> Option<Task<Message>> {
        self.table_cell_step(if forward { CellStep::Next } else { CellStep::Previous })
    }

    /// Enter in a cell's editor: the typed text goes into the cell and the
    /// editor moves to the cell below (same column, below a merged cell's
    /// span); Enter on the last row only commits and closes the editor.
    pub(in crate::app) fn table_cell_enter(&mut self) -> Option<Task<Message>> {
        self.table_cell_step(CellStep::Down)
    }

    /// After the data type question: "change the type" puts the value in
    /// with the cell's type changed to the value's (one undo step) and goes
    /// on as the key that ended the editing would; "change the value"
    /// reopens the cell's editor with the value.
    fn resume_cell_entry(&mut self, entry: CellEntry, change_type: bool) -> Task<Message> {
        let i = self.active_tab;
        let Some(codec::EntityType::Table(table)) =
            self.tabs[i].scene.document.get_entity(entry.handle)
        else {
            return Task::none();
        };
        let mut command = crate::modules::annotate::table_cmd::TableCellEditCommand::new(
            entry.handle,
            table,
            entry.row,
            entry.column,
        );
        if change_type {
            let (data_type, unit_type, picture) = crate::entities::table::entered_value_type(&entry.text);
            command = command.with_value_type(data_type, unit_type, &picture);
        }
        use crate::command::CadCommand;
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.command_line.input = entry.text;
        // The value comes back selected: typing replaces it.
        if !change_type {
            let input = iced::widget::Id::new(crate::ui::command_line::CMD_INPUT_ID);
            return self.focus_cmd_input().chain(iced::widget::operation::select_all(input));
        }
        self.table_cell_entry_typed = true;
        self.table_cell_step(entry.step).unwrap_or_else(Task::none)
    }

    fn table_cell_step(&mut self, step: CellStep) -> Option<Task<Message>> {
        let forward = step == CellStep::Next;
        let i = self.active_tab;
        let (handle, row, column) = self.tabs[i].active_cmd.as_ref()?.table_cell()?;
        // A trailing Space is not content (Space types a literal space).
        let text = std::mem::take(&mut self.command_line.input)
            .trim_end()
            .to_string();
        // A value that does not read as the cell's data type is not put in:
        // a question asks whether the type changes, the value is edited
        // again, or the value is dropped.
        let fits = match self.tabs[i].scene.document.get_entity(handle) {
            Some(codec::EntityType::Table(table)) => crate::entities::table::typed_text_fits(
                &self.tabs[i].scene.document,
                table,
                row,
                column,
                &text,
            ),
            _ => true,
        };
        if !fits && !self.table_cell_entry_typed {
            self.tabs[i].active_cmd = None;
            self.table_cell_entry = Some(CellEntry { handle, row, column, text, step });
            self.active_modal = Some(crate::app::ModalKind::TableCellEntry);
            self.reset_modal_geometry();
            return Some(Task::none());
        }
        self.table_cell_entry_typed = false;
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
        match step {
            CellStep::Next => next = (start + 1..count).find(|&index| owner(index)),
            CellStep::Previous if start > 0 => {
                next = (0..start).rev().find(|&index| owner(index))
            }
            CellStep::Previous => {}
            CellStep::Down => {
                let (_, _, r1, _) =
                    crate::entities::table::widen_to_merged(table, (row, column, row, column));
                if r1 + 1 >= table.row_count() {
                    return Some(committed);
                }
                let (r0, c0, _, _) = crate::entities::table::widen_to_merged(
                    table,
                    (r1 + 1, column, r1 + 1, column),
                );
                next = Some(r0 * columns + c0);
            }
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

/// A value typed into a cell's editor that is not of the cell's data type.
pub(in crate::app) struct CellEntry {
    handle: codec::Handle,
    row: usize,
    column: usize,
    pub(in crate::app) text: String,
    step: CellStep,
}

impl CellEntry {
    /// The cell's name, as the column letters and row number show it.
    pub(in crate::app) fn cell_name(&self) -> String {
        format!("{}{}", super::field::column_letters(self.column), self.row + 1)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellStep {
    Next,
    Previous,
    Down,
}

/// Standard lineweights the Cell Borders window lists, in 1/100 mm.
const LINEWEIGHTS: [i16; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200,
    211,
];

fn lineweight_name(weight: codec::types::LineWeight) -> String {
    match weight {
        codec::types::LineWeight::ByLayer => "ByLayer".into(),
        codec::types::LineWeight::ByBlock => "ByBlock".into(),
        codec::types::LineWeight::Value(v) if v >= 0 => format!("{:.2} mm", v as f64 / 100.0),
        _ => "Default".into(),
    }
}

fn parse_lineweight(text: &str) -> codec::types::LineWeight {
    match text.trim() {
        "ByLayer" => codec::types::LineWeight::ByLayer,
        "ByBlock" => codec::types::LineWeight::ByBlock,
        "Default" => codec::types::LineWeight::Default,
        other => other
            .trim_end_matches("mm")
            .trim()
            .parse::<f64>()
            .map(|mm| codec::types::LineWeight::Value((mm * 100.0).round() as i16))
            .unwrap_or(codec::types::LineWeight::ByBlock),
    }
}

/// The alignment keywords the Alignment tool offers, as the style code.
const ALIGNMENTS: [(&str, &str, i32); 9] = [
    ("Top Left", "TL", 1),
    ("Top Center", "TC", 2),
    ("Top Right", "TR", 3),
    ("Middle Left", "ML", 4),
    ("Middle Center", "MC", 5),
    ("Middle Right", "MR", 6),
    ("Bottom Left", "BL", 7),
    ("Bottom Center", "BC", 8),
    ("Bottom Right", "BR", 9),
];

/// The data format keywords the Data Format tool offers.
const FORMATS: [(&str, &str); 9] = [
    ("Angle", "ANGLE"),
    ("Currency", "CURRENCY"),
    ("Date", "DATE"),
    ("Decimal Number", "DECIMAL"),
    ("General", "GENERAL"),
    ("Percentage", "PERCENTAGE"),
    ("Point", "POINT"),
    ("Text", "TEXT"),
    ("Whole Number", "WHOLE"),
];

impl OpenCADStudio {
    /// One edit of the picked cells' table as one undo step; the picked
    /// range stays picked.
    fn edit_picked_table(&mut self, i: usize, label: &str, edit: impl FnOnce(&mut Table) -> bool) {
        let Some(cells) = self.tabs[i].scene.table_cells else {
            return;
        };
        let handle = cells.table;
        if self.tabs[i].scene.is_layer_locked(handle) {
            self.command_line.push_info("1 was on a locked layer.");
            return;
        }
        let mut table = match self.tabs[i].scene.document.get_entity(handle) {
            Some(codec::EntityType::Table(table)) => (**table).clone(),
            _ => return,
        };
        if !edit(&mut table) {
            return;
        }
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
        self.tabs[i].scene.select_table_cells(cells);
        self.refresh_properties();
    }

    /// Tools of the picked cells beyond rows, columns, merging and locking.
    pub(super) fn dispatch_table_cell_tools(
        &mut self,
        cmd: &str,
        i: usize,
        (r0, c0): (usize, usize),
    ) -> Option<Task<Message>> {
        use crate::command::{CadCommand, KeywordCommand};
        let (verb, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
        let rest = rest.trim();
        let table_handle = self.tabs[i].scene.table_cells?.table;
        let format_editable = |cell: &TableCell| {
            !cell
                .state
                .intersects(CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY)
        };
        let content_editable = |cell: &TableCell| {
            !cell
                .state
                .intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY)
        };
        let start = |app: &mut Self, command: Box<dyn CadCommand>| {
            app.command_line.push_info(&command.prompt());
            app.tabs[i].active_cmd = Some(command);
        };
        match verb {
            // Match Cell: the first picked cell's formatting goes to each
            // cell picked next, until Enter.
            "_TCELLMATCH" if rest.is_empty() => {
                start(
                    self,
                    Box::new(crate::modules::annotate::table_cmd::CellPointsCommand::new(
                        "_TCELLMATCH",
                        vec!["Select destination cell:"],
                    )),
                );
            }
            "_TCELLMATCH" => {
                let point = parse_points(rest).first().copied()?;
                let target = self.tabs[i]
                    .scene
                    .table_cell_pick(point, 0.0)
                    .filter(|(hit, _)| *hit == table_handle)
                    .map(|(_, cell)| cell);
                if let Some((row, column)) = target {
                    self.edit_picked_table(i, "TABLE MATCH CELL", |table| {
                        let Some(source) = table.cell(r0, c0).cloned() else {
                            return false;
                        };
                        let Some(target) = table.cell_mut(row, column) else {
                            return false;
                        };
                        if !format_editable(target) {
                            return false;
                        }
                        target.style = source.style.clone();
                        target.style_id = source.style_id;
                        true
                    });
                }
                start(
                    self,
                    Box::new(crate::modules::annotate::table_cmd::CellPointsCommand::new(
                        "_TCELLMATCH",
                        vec!["Select destination cell:"],
                    )),
                );
            }
            "_TCELLALIGN" if rest.is_empty() => {
                let command = KeywordCommand::new(
                    "_TCELLALIGN",
                    "Enter cell alignment [TL/TC/TR/ML/MC/MR/BL/BC/BR]:",
                    ALIGNMENTS
                        .iter()
                        .map(|(label, keyword, _)| (*label, *keyword, None))
                        .collect(),
                );
                start(self, Box::new(command));
            }
            "_TCELLALIGN" => {
                let code = ALIGNMENTS
                    .iter()
                    .find(|(_, keyword, _)| keyword.eq_ignore_ascii_case(rest))
                    .map(|(_, _, code)| *code)?;
                let picked = self.picked_cell_positions(i);
                self.edit_picked_table(i, "TABLE CELL ALIGNMENT", |table| {
                    let mut changed = false;
                    for &(row, column) in &picked {
                        if let Some(cell) = table.cell_mut(row, column).filter(|c| format_editable(c)) {
                            let style = cell.style.get_or_insert_with(Default::default);
                            style.alignment = code;
                            style.property_flags.insert(
                                codec::entities::table::CellStylePropertyFlags::ALIGNMENT,
                            );
                            changed = true;
                        }
                    }
                    changed
                });
            }
            // Data Format and Insert Block open their windows; the keyword
            // forms stay for scripts.
            "_TCELLFORMAT" if rest.is_empty() => self.open_cell_format(i),
            "_TCELLFORMAT" => {
                let name = FORMATS
                    .iter()
                    .find(|(_, keyword)| keyword.eq_ignore_ascii_case(rest))
                    .map(|(name, _)| *name)?;
                let (_, data_type, unit_type) = crate::entities::table::CELL_DATA_CATEGORIES
                    .iter()
                    .find(|(category, _, _)| *category == name)
                    .copied()?;
                let picked = self.picked_cell_positions(i);
                self.edit_picked_table(i, "TABLE CELL FORMAT", |table| {
                    let mut changed = false;
                    for &(row, column) in &picked {
                        if let Some(cell) = table.cell_mut(row, column).filter(|c| format_editable(c)) {
                            crate::entities::table::release_content_format(
                                cell,
                                codec::entities::table::CellStylePropertyFlags::DATA_TYPE,
                            );
                            let style = cell.style.get_or_insert_with(Default::default);
                            style.value_data_type = data_type;
                            style.value_unit_type = unit_type;
                            style.property_flags.insert(
                                codec::entities::table::CellStylePropertyFlags::DATA_TYPE,
                            );
                            changed = true;
                        }
                    }
                    changed
                });
            }
            // Insert Block: the named block becomes the content of the picked
            // cells; `?` lists the blocks.
            "_TCELLBLOCK" if rest.is_empty() => self.open_cell_block(i),
            "_TCELLBLOCK" => {
                let document = &self.tabs[i].scene.document;
                if rest == "?" {
                    let mut names: Vec<String> = document
                        .block_records
                        .iter()
                        .filter(|record| !record.name.starts_with('*'))
                        .map(|record| record.name.clone())
                        .collect();
                    names.sort_by_key(|name| name.to_ascii_lowercase());
                    for name in names {
                        self.command_line.push_output(&name);
                    }
                    return Some(Task::none());
                }
                let Some(block) = document
                    .block_records
                    .iter()
                    .find(|record| record.name.eq_ignore_ascii_case(rest) && !record.name.starts_with('*'))
                    .map(|record| record.handle)
                else {
                    self.command_line
                        .push_error(crate::tf!("Block \"{}\" not found.", rest).as_ref());
                    return Some(Task::none());
                };
                let picked = self.picked_cell_positions(i);
                self.edit_picked_table(i, "TABLE CELL BLOCK", |table| {
                    let mut changed = false;
                    for &(row, column) in &picked {
                        if let Some(cell) = table.cell_mut(row, column).filter(|c| content_editable(c)) {
                            cell.contents =
                                vec![codec::entities::table::CellContent::block(block)];
                            cell.cell_type = codec::entities::table::CellType::Block;
                            changed = true;
                        }
                    }
                    changed
                });
            }
            "_TCELLFIELD" => {
                self.open_field_dialog(crate::ui::window::field_dialog::FieldTarget::TableCell(
                    table_handle,
                    r0,
                    c0,
                ));
            }
            // Formula: Sum / Average / Count over a picked range of cells,
            // a reference to one cell, or an equation typed in the cell.
            "_TCELLFORMULA" if rest.is_empty() => {
                let command = KeywordCommand::new(
                    "_TCELLFORMULA",
                    "Enter a formula [Sum/Average/Count/Cell/Equation]:",
                    vec![
                        ("Sum", "SUM", None),
                        ("Average", "AVERAGE", None),
                        ("Count", "COUNT", None),
                        ("Cell", "CELL", None),
                        ("Equation", "EQUATION", None),
                    ],
                );
                start(self, Box::new(command));
            }
            "_TCELLFORMULA" => {
                let (function, points) = rest.split_once(' ').unwrap_or((rest, ""));
                let function = function.to_ascii_uppercase();
                let points = parse_points(points);
                let cell_at = |app: &Self, point: glam::DVec3| {
                    app.tabs[i]
                        .scene
                        .table_cell_pick(point, 0.0)
                        .filter(|(hit, _)| *hit == table_handle)
                        .map(|(_, cell)| cell)
                };
                let label = |(row, column): (usize, usize)| {
                    format!("{}{}", crate::entities::table::column_label(column), row + 1)
                };
                let formula = match (function.as_str(), points.as_slice()) {
                    ("EQUATION", _) => {
                        self.edit_picked_table(i, "TABLE CELL FORMULA", |table| {
                            table
                                .cell_mut(r0, c0)
                                .filter(|c| content_editable(c))
                                .map(|cell| cell.set_text("="))
                                .is_some()
                        });
                        let Some(codec::EntityType::Table(table)) =
                            self.tabs[i].scene.document.get_entity(table_handle)
                        else {
                            return Some(Task::none());
                        };
                        let command = crate::modules::annotate::table_cmd::TableCellEditCommand::new(
                            table_handle,
                            table,
                            r0,
                            c0,
                        );
                        start(self, Box::new(command));
                        return Some(Task::none());
                    }
                    ("CELL", []) => {
                        start(
                            self,
                            Box::new(crate::modules::annotate::table_cmd::CellPointsCommand::new(
                                "_TCELLFORMULA CELL",
                                vec!["Select table cell:"],
                            )),
                        );
                        return Some(Task::none());
                    }
                    ("SUM" | "AVERAGE" | "COUNT", []) => {
                        let name: &'static str = match function.as_str() {
                            "SUM" => "_TCELLFORMULA SUM",
                            "AVERAGE" => "_TCELLFORMULA AVERAGE",
                            _ => "_TCELLFORMULA COUNT",
                        };
                        start(
                            self,
                            Box::new(crate::modules::annotate::table_cmd::CellPointsCommand::new(
                                name,
                                vec![
                                    "Select first corner of table cell range:",
                                    "Select second corner of table cell range:",
                                ],
                            )),
                        );
                        return Some(Task::none());
                    }
                    ("CELL", [point, ..]) => {
                        let Some(cell) = cell_at(self, *point) else {
                            self.command_line.push_error(crate::t!("Invalid cell.").as_ref());
                            return Some(Task::none());
                        };
                        format!("={}", label(cell))
                    }
                    (_, [first, second, ..]) => {
                        let (Some(a), Some(b)) = (cell_at(self, *first), cell_at(self, *second))
                        else {
                            self.command_line.push_error(crate::t!("Invalid cell.").as_ref());
                            return Some(Task::none());
                        };
                        let from = (a.0.min(b.0), a.1.min(b.1));
                        let to = (a.0.max(b.0), a.1.max(b.1));
                        let name = match function.as_str() {
                            "SUM" => "Sum",
                            "AVERAGE" => "Average",
                            _ => "Count",
                        };
                        format!("={name}({}:{})", label(from), label(to))
                    }
                    _ => return None,
                };
                self.edit_picked_table(i, "TABLE CELL FORMULA", |table| {
                    table
                        .cell_mut(r0, c0)
                        .filter(|c| content_editable(c))
                        .map(|cell| cell.set_text(&formula))
                        .is_some()
                });
            }
            "_TCELLBORDERS" => self.open_cell_borders(i),
            "_TCELLCONTENTS" => self.open_cell_contents(i, (r0, c0)),
            // Link Cell: the data link chosen in the Data Link Manager feeds
            // the picked cells from their first one.
            "_TCELLLINK" => {
                self.data_link_cell_target = true;
                self.open_data_link_manager(false);
            }
            "_TCELLDOWNLOAD" => {
                return Some(self.update(Message::Command("DATALINKUPDATE _RUN U SEL".into())));
            }
            _ => return None,
        }
        Some(Task::none())
    }

    /// The data link the manager closed on, written into the picked cells
    /// from the first one, each cell keeping the link.
    pub(in crate::app) fn link_picked_cells(&mut self, link: codec::Handle) {
        let i = self.active_tab;
        let rows = match crate::app::annotation_data::read_data_link(
            &self.tabs[i].scene.document,
            link,
        ) {
            Ok(rows) => rows,
            Err(error) => {
                self.command_line.push_error(&error);
                return;
            }
        };
        let Some(cells) = self.tabs[i].scene.table_cells else {
            return;
        };
        let (r0, c0, _, _) = cells.range();
        let table_handle = cells.table;
        self.edit_picked_table(i, "TABLE LINK CELL", |table| {
            let mut changed = false;
            for (dr, values) in rows.iter().enumerate() {
                for (dc, value) in values.iter().enumerate() {
                    if let Some(cell) = table.cell_mut(r0 + dr, c0 + dc) {
                        cell.set_text(value);
                        cell.data_link_handle = Some(link);
                        cell.has_linked_data = true;
                        changed = true;
                    }
                }
            }
            changed
        });
        crate::app::annotation_data::connect_data_link(
            &mut self.tabs[i].scene.document,
            link,
            table_handle,
        );
    }

    fn open_cell_borders(&mut self, i: usize) {
        use crate::ui::window::table_cell_dialog::{BorderSet, CellBordersState};
        let Some(cells) = self.tabs[i].scene.table_cells else {
            return;
        };
        let document = &self.tabs[i].scene.document;
        let ((r0, c0, r1, c1), spacing) = match document.get_entity(cells.table) {
            Some(codec::EntityType::Table(table)) => (
                crate::entities::table::widen_to_merged(table, cells.range()),
                // A quarter of the data text height, as the reference opens it.
                0.25 * crate::entities::table::data_text_height(table, document),
            ),
            _ => return,
        };
        let mut lineweights = vec!["ByLayer".to_string(), "ByBlock".into()];
        lineweights.extend(
            LINEWEIGHTS
                .iter()
                .map(|v| lineweight_name(codec::types::LineWeight::Value(*v))),
        );
        let mut linetypes = vec!["ByBlock".to_string(), "ByLayer".into()];
        linetypes.extend(
            document
                .line_types
                .iter()
                .map(|lt| lt.name.clone())
                .filter(|name| {
                    !name.eq_ignore_ascii_case("ByBlock") && !name.eq_ignore_ascii_case("ByLayer")
                }),
        );
        self.table_cell_borders = Some(CellBordersState {
            lineweights,
            lineweight: "ByBlock".into(),
            linetypes,
            linetype: "ByLayer".into(),
            colors: crate::entities::table::CELL_COLOR_NAMES
                .iter()
                .map(|name| name.to_string())
                .collect(),
            color: "ByBlock".into(),
            double: false,
            spacing: crate::entities::common::format_length(spacing),
            sets: vec![BorderSet::All],
            rows: r1 + 1 - r0,
            columns: c1 + 1 - c0,
        });
        self.active_modal = Some(crate::app::ModalKind::TableCellBorders);
        self.reset_modal_geometry();
    }

    fn open_cell_contents(&mut self, i: usize, (row, column): (usize, usize)) {
        use crate::ui::window::table_cell_dialog::CellContentsState;
        let document = &self.tabs[i].scene.document;
        let Some(cells) = self.tabs[i].scene.table_cells else {
            return;
        };
        let Some(codec::EntityType::Table(table)) = document.get_entity(cells.table) else {
            return;
        };
        let Some(cell) = table.cell(row, column) else {
            return;
        };
        let items: Vec<String> = cell
            .contents
            .iter()
            .map(|content| match content.block_handle {
                Some(block) => {
                    let name = document
                        .block_records
                        .iter()
                        .find(|record| record.handle == block)
                        .map(|record| record.name.clone())
                        .unwrap_or_default();
                    crate::tr!("table-cells", "block-item", name = name)
                }
                None => crate::tr!("table-cells", "text-content"),
            })
            .collect();
        let style = cell.style.as_ref().filter(|style| {
            style
                .property_flags
                .contains(codec::entities::table::CellStylePropertyFlags::CONTENT_LAYOUT)
        });
        let layout = style
            .map(|style| style.layout_flags.bits())
            .filter(|bits| [1, 2, 4].contains(bits))
            .unwrap_or(1);
        let spacing = style
            .map(|style| {
                if layout == 4 {
                    style.vertical_spacing
                } else {
                    style.horizontal_spacing
                }
            })
            .unwrap_or_else(|| crate::entities::table::data_text_height(table, document));
        self.table_cell_contents = Some(CellContentsState {
            order: (0..items.len()).collect(),
            items,
            selected: None,
            layout,
            spacing: crate::entities::common::format_length(spacing),
        });
        self.active_modal = Some(crate::app::ModalKind::TableCellContents);
        self.reset_modal_geometry();
    }

    pub(in crate::app) fn update_table_cell_dialog(
        &mut self,
        message: crate::ui::window::table_cell_dialog::TableCellDialogMsg,
    ) -> Task<Message> {
        use crate::ui::window::table_cell_dialog::{BorderSet, TableCellDialogMsg as M};
        let i = self.active_tab;
        // OK in the invalid data type warning converts the cells as the
        // format asked; values that read as no such type are emptied.
        if matches!(message, M::EntryType | M::EntryValue)
            && self.active_modal == Some(crate::app::ModalKind::TableCellEntry)
        {
            if let Some(entry) = self.table_cell_entry.take() {
                self.close_active_modal();
                return self.resume_cell_entry(entry, matches!(message, M::EntryType));
            }
            return Task::none();
        }
        if matches!(message, M::WarnOk)
            && self.active_modal == Some(crate::app::ModalKind::TableCellWarning)
        {
            if let Some((state, _)) = self.table_cell_warning.take() {
                self.close_active_modal();
                self.apply_cell_format(i, state, true);
            }
            return Task::none();
        }
        if let Some(state) = self.table_cell_borders.as_mut().filter(|_| {
            self.active_modal == Some(crate::app::ModalKind::TableCellBorders)
        }) {
            match message {
                M::Lineweight(value) => state.lineweight = value,
                M::Linetype(value) => state.linetype = value,
                M::Color(value) => state.color = value,
                M::Double(on) => state.double = on,
                M::Spacing(value) => state.spacing = value,
                M::Edges(set) => match set {
                    BorderSet::All | BorderSet::None => state.sets = vec![set],
                    _ => {
                        state.sets.retain(|s| !matches!(s, BorderSet::All | BorderSet::None));
                        if let Some(at) = state.sets.iter().position(|s| *s == set) {
                            state.sets.remove(at);
                        } else {
                            state.sets.push(set);
                        }
                    }
                },
                M::Help => self.command_line.push_info(
                    crate::tr!("table-cells", "borders-help").as_str(),
                ),
                M::Ok => {
                    let state = self.table_cell_borders.take().expect("open");
                    self.close_active_modal();
                    self.apply_cell_borders(i, &state);
                }
                _ => {}
            }
            return Task::none();
        }
        if let Some(state) = self.table_cell_block.as_mut().filter(|_| {
            self.active_modal == Some(crate::app::ModalKind::TableCellBlock)
        }) {
            match message {
                M::BlockName(name) => {
                    state.name = name;
                    state.path.clear();
                }
                M::BlockPath(path) => {
                    state.name = std::path::Path::new(&path)
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    state.path = path;
                }
                M::BlockBrowse => {
                    return Task::perform(
                        async {
                            rfd::AsyncFileDialog::new()
                                .add_filter("Drawing", &["dwg", "dxf"])
                                .pick_file()
                                .await
                                .map(|h| crate::sys::handle_path(&h))
                        },
                        |path| match path {
                            Some(path) => Message::TableCellDialog(M::BlockPath(
                                path.to_string_lossy().into_owned(),
                            )),
                            None => Message::Noop,
                        },
                    );
                }
                M::Align(alignment) => state.alignment = alignment.clamp(1, 9),
                M::BlockScale(value) => state.scale = value,
                M::AutoFit(on) => state.autofit = on,
                M::BlockRotation(value) => state.rotation = value,
                M::Help => self
                    .command_line
                    .push_info(crate::tr!("table-cells", "block-help").as_str()),
                M::Ok => {
                    let state = self.table_cell_block.take().expect("open");
                    self.close_active_modal();
                    self.apply_cell_block(i, state);
                }
                _ => {}
            }
            return Task::none();
        }
        if let Some(state) = self.table_cell_format.as_mut().filter(|_| {
            self.active_modal == Some(crate::app::ModalKind::TableCellFormat)
        }) {
            match message {
                M::FormatKind(kind) => {
                    state.kind = kind.min(8);
                    state.unit = 0;
                    state.precision = if matches!(state.kind, 1 | 5) { 3 } else { 0 };
                    state.append = state.kind == 5;
                }
                M::FormatUnit(unit) => state.unit = unit,
                M::Precision(value) => {
                    state.precision = if value.contains('.') {
                        value.len() - 1
                    } else if value == "0" {
                        1
                    } else {
                        0
                    };
                }
                M::Symbol(value) => state.symbol = value,
                M::Append(on) => state.append = on,
                M::Negative(on) => state.negative_parens = on,
                M::Separator(index) => state.separator = index.min(2),
                M::Axis(axis) => {
                    if let Some(on) = state.axes.get_mut(axis) {
                        *on = !*on;
                    }
                }
                M::DateFormat(value) => state.date = value,
                M::Conversion(value) => state.conversion = value,
                M::Prefix(value) => state.prefix = value,
                M::Suffix(value) => state.suffix = value,
                M::DecimalSeparator(index) => state.decimal = index.min(2),
                M::ThousandsSeparator(index) => state.thousands = index.min(3),
                M::Zero(bit) => state.zeros ^= bit,
                M::Help => self
                    .command_line
                    .push_info(crate::tr!("table-cells", "format-help").as_str()),
                M::Ok => {
                    let state = self.table_cell_format.take().expect("open");
                    self.close_active_modal();
                    self.apply_cell_format(i, state, false);
                    return Task::none();
                }
                _ => {}
            }
            let preview = format_preview(&self.tabs[i].scene.document, state);
            if let Some(state) = self.table_cell_format.as_mut() {
                state.preview = preview;
            }
            return Task::none();
        }
        if let Some(state) = self.table_cell_contents.as_mut() {
            match message {
                M::Pick(position) => state.selected = Some(position),
                M::Up => {
                    if let Some(p) = state.selected.filter(|p| *p > 0) {
                        state.order.swap(p, p - 1);
                        state.selected = Some(p - 1);
                    }
                }
                M::Down => {
                    if let Some(p) = state.selected.filter(|p| p + 1 < state.order.len()) {
                        state.order.swap(p, p + 1);
                        state.selected = Some(p + 1);
                    }
                }
                M::Delete => {
                    if let Some(p) = state.selected {
                        state.order.remove(p);
                        state.selected = (!state.order.is_empty())
                            .then(|| p.min(state.order.len() - 1));
                    }
                }
                M::Layout(bits) => state.layout = bits,
                M::ContentSpacing(value) => state.spacing = value,
                M::Help => self.command_line.push_info(
                    crate::tr!("table-cells", "contents-help").as_str(),
                ),
                M::Ok => {
                    let state = self.table_cell_contents.take().expect("open");
                    self.close_active_modal();
                    let Some(cells) = self.tabs[i].scene.table_cells else {
                        return Task::none();
                    };
                    let (row, column) = {
                        let Some(codec::EntityType::Table(table)) =
                            self.tabs[i].scene.document.get_entity(cells.table)
                        else {
                            return Task::none();
                        };
                        let (r0, c0, _, _) =
                            crate::entities::table::widen_to_merged(table, cells.range());
                        (r0, c0)
                    };
                    let spacing = crate::entities::common::parse_f64(&state.spacing)
                        .filter(|v| *v >= 0.0)
                        .unwrap_or(0.0);
                    self.edit_picked_table(i, "TABLE CELL CONTENTS", |table| {
                        let Some(cell) = table.cell_mut(row, column) else {
                            return false;
                        };
                        let contents = std::mem::take(&mut cell.contents);
                        cell.contents = state
                            .order
                            .iter()
                            .filter_map(|&index| contents.get(index).cloned())
                            .collect();
                        if cell.contents.is_empty() {
                            cell.set_text("");
                            cell.cell_type = codec::entities::table::CellType::Text;
                        } else if cell.contents.iter().all(|c| c.block_handle.is_none()) {
                            cell.cell_type = codec::entities::table::CellType::Text;
                        }
                        let style = cell.style.get_or_insert_with(Default::default);
                        style.layout_flags =
                            codec::entities::table::ContentLayoutFlags::from_bits_truncate(
                                state.layout,
                            );
                        style.horizontal_spacing = spacing;
                        style.vertical_spacing = spacing;
                        style.property_flags.insert(
                            codec::entities::table::CellStylePropertyFlags::CONTENT_LAYOUT
                                | codec::entities::table::CellStylePropertyFlags::MARGIN_HORIZONTAL_SPACING
                                | codec::entities::table::CellStylePropertyFlags::MARGIN_VERTICAL_SPACING,
                        );
                        true
                    });
                }
                _ => {}
            }
        }
        Task::none()
    }

    /// The Cell Borders settings on the chosen edges of the picked range.
    /// An edge between two cells is set on both of them, so it draws the
    /// same whichever cell draws it.
    fn apply_cell_borders(
        &mut self,
        i: usize,
        state: &crate::ui::window::table_cell_dialog::CellBordersState,
    ) {
        use crate::entities::table::TableCellBordersExt;
        use crate::ui::window::table_cell_dialog::BorderSet;
        use codec::entities::table::{BorderPropertyFlags as B, BorderType, CellEdgeFlags as E};
        let Some(cells) = self.tabs[i].scene.table_cells else {
            return;
        };
        let picked = self.picked_cell_positions(i);
        let document = &self.tabs[i].scene.document;
        let Some(codec::EntityType::Table(table)) = document.get_entity(cells.table) else {
            return;
        };
        let (r0, c0, r1, c1) = crate::entities::table::widen_to_merged(table, cells.range());
        // Each picked cell with the last row and column its merge reaches.
        let picked: Vec<(usize, usize, usize, usize)> = picked
            .into_iter()
            .map(|(row, column)| {
                let (_, _, last_row, last_column) =
                    crate::entities::table::widen_to_merged(table, (row, column, row, column));
                (row, column, last_row, last_column)
            })
            .collect();
        let linetype = document
            .line_types
            .iter()
            .find(|lt| lt.name.eq_ignore_ascii_case(&state.linetype))
            .map(|lt| lt.handle);
        let color = crate::entities::table::parse_cell_color(&state.color)
            .unwrap_or(codec::types::Color::ByBlock);
        let weight = parse_lineweight(&state.lineweight);
        let spacing = crate::entities::common::parse_f64(&state.spacing).unwrap_or(0.0);
        let hide = state.sets.contains(&BorderSet::None);
        let sets = if hide { vec![BorderSet::All] } else { state.sets.clone() };
        let has = |set: BorderSet| sets.contains(&BorderSet::All) || sets.contains(&set);
        let chosen = |row: usize, column: usize, last_row: usize, last_column: usize| -> E {
            let mut edges = E::empty();
            let outside_or = |outside: bool, side: BorderSet, inner: BorderSet| {
                if outside {
                    has(BorderSet::Outside) || has(side)
                } else {
                    has(BorderSet::Inside) || has(inner)
                }
            };
            if outside_or(row == r0, BorderSet::Top, BorderSet::InsideHorizontal) {
                edges |= E::TOP;
            }
            if outside_or(last_row == r1, BorderSet::Bottom, BorderSet::InsideHorizontal) {
                edges |= E::BOTTOM;
            }
            if outside_or(column == c0, BorderSet::Left, BorderSet::InsideVertical) {
                edges |= E::LEFT;
            }
            if outside_or(last_column == c1, BorderSet::Right, BorderSet::InsideVertical) {
                edges |= E::RIGHT;
            }
            edges
        };
        self.edit_picked_table(i, "TABLE CELL BORDERS", |table| {
            let rows = table.rows.len();
            let columns = table.columns.len();
            let set = |border: &mut codec::entities::table::CellBorder| {
                border.invisible = hide;
                if !hide {
                    border.line_weight = weight;
                    border.color = color;
                    border.line_type_handle = linetype;
                    border.border_type = if state.double {
                        BorderType::Double
                    } else {
                        BorderType::Single
                    };
                    border.double_spacing = spacing;
                }
                B::INVISIBILITY | B::LINE_WEIGHT | B::COLOR | B::LINE_TYPE | B::BORDER_TYPE
            };
            for &(row, column, last_row, last_column) in &picked {
                let edges = chosen(row, column, last_row, last_column);
                table.edit_cell_borders_on(row, column, edges, set);
                // The neighbours across an outer edge (all along a merged
                // cell's side) draw the same line.
                for c in column..=last_column {
                    if edges.contains(E::TOP) && row > 0 {
                        table.edit_cell_borders_on(row - 1, c, E::BOTTOM, set);
                    }
                    if edges.contains(E::BOTTOM) && last_row + 1 < rows {
                        table.edit_cell_borders_on(last_row + 1, c, E::TOP, set);
                    }
                }
                for r in row..=last_row {
                    if edges.contains(E::LEFT) && column > 0 {
                        table.edit_cell_borders_on(r, column - 1, E::RIGHT, set);
                    }
                    if edges.contains(E::RIGHT) && last_column + 1 < columns {
                        table.edit_cell_borders_on(r, last_column + 1, E::LEFT, set);
                    }
                }
            }
            true
        });
    }
}

/// Points from `x y z x y z …`.
fn parse_points(text: &str) -> Vec<glam::DVec3> {
    let values: Vec<f64> = text
        .split_whitespace()
        .filter_map(|value| value.parse().ok())
        .collect();
    values
        .chunks_exact(3)
        .map(|v| glam::DVec3::new(v[0], v[1], v[2]))
        .collect()
}

/// The preview of the Cell Data Format settings: a sample of the chosen data
/// type, formatted as the cells will read.
fn format_preview(
    document: &codec::Document,
    state: &crate::ui::window::table_cell_dialog::CellFormatState,
) -> String {
    let picture = state.picture();
    match state.kind {
        0 => document.format_with_picture(
            state.sample.unwrap_or(2.103),
            if picture.is_empty() { "%au5" } else { &picture },
        ),
        1 | 3 | 5 => {
            let sample = state.sample.unwrap_or(match state.kind {
                1 => -1234.1,
                3 => 845.34786,
                _ => 12.34,
            });
            if picture.is_empty() {
                sample.to_string()
            } else {
                document.format_with_picture(sample, &picture)
            }
        }
        2 => {
            let picture = if picture.is_empty() { "M/d/yyyy" } else { &picture };
            codec::fields::format_dt((2026, 11, 2, 14, 30, 0), picture)
        }
        6 => document.format_point_with_picture([2.58, 5.37, 0.0], &picture),
        8 => "1234".to_string(),
        _ => crate::tr!("table-cells", "sample-text"),
    }
}

impl OpenCADStudio {
    fn open_cell_block(&mut self, i: usize) {
        use crate::ui::window::table_cell_dialog::CellBlockState;
        let Some(cells) = self.tabs[i].scene.table_cells else {
            return;
        };
        let document = &self.tabs[i].scene.document;
        let mut names: Vec<String> = document
            .block_records
            .iter()
            .filter(|record| !record.name.starts_with('*'))
            .filter(|record| {
                !record.name.eq_ignore_ascii_case("Model_Space")
                    && !record.name.to_ascii_lowercase().starts_with("paper_space")
            })
            .map(|record| record.name.clone())
            .collect();
        names.sort_by_key(|name| name.to_ascii_lowercase());
        // The picked cell's block, when it already holds one.
        let current = match document.get_entity(cells.table) {
            Some(codec::EntityType::Table(table)) => {
                let (r0, c0, _, _) = crate::entities::table::widen_to_merged(table, cells.range());
                table.cell(r0, c0).and_then(|cell| {
                    let content = cell.contents.iter().find(|c| c.block_handle.is_some())?;
                    let name = document
                        .block_records
                        .iter()
                        .find(|record| Some(record.handle) == content.block_handle)?
                        .name
                        .clone();
                    Some((name, content.scale, content.rotation, cell.auto_fit))
                })
            }
            _ => None,
        };
        let (name, scale, rotation, autofit) = current.unwrap_or_else(|| {
            (names.first().cloned().unwrap_or_default(), 1.0, 0.0, true)
        });
        self.table_cell_block = Some(CellBlockState {
            names,
            name,
            path: String::new(),
            alignment: 1,
            scale: crate::entities::common::format_length(if scale > 0.0 { scale } else { 1.0 }),
            autofit,
            rotation: crate::entities::common::format_length(rotation.to_degrees()),
        });
        self.active_modal = Some(crate::app::ModalKind::TableCellBlock);
        self.reset_modal_geometry();
    }

    fn open_cell_format(&mut self, i: usize) {
        use crate::ui::window::table_cell_dialog::{CellFormatState, FORMAT_KINDS};
        let Some(cells) = self.tabs[i].scene.table_cells else {
            return;
        };
        let document = &self.tabs[i].scene.document;
        let Some(codec::EntityType::Table(table)) = document.get_entity(cells.table) else {
            return;
        };
        let (r0, c0, _, _) = crate::entities::table::widen_to_merged(table, cells.range());
        let first = table.rows.get(r0).zip(table.cell(r0, c0));
        let picture = first
            .map(|(row, cell)| crate::entities::table::cell_value_picture(row, c0, table, cell))
            .unwrap_or_default();
        let sample = table
            .cell(r0, c0)
            .and_then(|cell| cell.contents.first())
            .filter(|content| {
                matches!(
                    content.value.value_type,
                    codec::entities::table::CellValueType::Long
                        | codec::entities::table::CellValueType::Double
                )
            })
            .map(|content| content.value.numeric_value);
        let category = first
            .and_then(|(row, cell)| crate::entities::table::cell_value_type(row, c0, table, cell))
            .map(|(data_type, unit_type)| {
                crate::entities::table::cell_data_category(data_type, unit_type)
            })
            .unwrap_or("General");
        let kind = FORMAT_KINDS.iter().position(|name| *name == category).unwrap_or(4);
        let mut state = CellFormatState {
            kind,
            unit: 0,
            precision: if matches!(kind, 1 | 5) { 3 } else { 0 },
            symbol: crate::entities::field::os_currency_symbol(),
            append: kind == 5,
            negative_parens: false,
            separator: 0,
            axes: [true; 3],
            date: String::new(),
            sample,
            preview: String::new(),
            conversion: "1".into(),
            prefix: String::new(),
            suffix: String::new(),
            decimal: 0,
            thousands: 0,
            zeros: 0,
        };
        state.read_picture(&picture);
        state.preview = format_preview(document, &state);
        self.table_cell_format = Some(state);
        self.active_modal = Some(crate::app::ModalKind::TableCellFormat);
        self.reset_modal_geometry();
    }

    /// The Cell Block settings on the picked cells: the block as their
    /// content, the cell alignment, and the scale (or fitted to the cell)
    /// and rotation. A block that does not fit grows its row.
    fn apply_cell_block(&mut self, i: usize, state: crate::ui::window::table_cell_dialog::CellBlockState) {
        let name = if state.path.is_empty() {
            state.name.clone()
        } else {
            match self.import_file_as_block(std::path::PathBuf::from(&state.path)) {
                Ok(name) => name,
                Err(error) => {
                    self.command_line.push_error(&error);
                    return;
                }
            }
        };
        let document = &self.tabs[i].scene.document;
        let Some(record) = document
            .block_records
            .iter()
            .find(|record| record.name.eq_ignore_ascii_case(&name))
        else {
            self.command_line
                .push_error(crate::tf!("Block \"{}\" not found.", name).as_ref());
            return;
        };
        let block = record.handle;
        let (mut min, mut max) = (glam::DVec2::splat(f64::MAX), glam::DVec2::splat(f64::MIN));
        for handle in &record.entity_handles {
            if let Some(entity) = document.get_entity(*handle) {
                let bounds = entity.as_entity().bounding_box();
                if bounds.min.x.is_finite() && bounds.max.x.is_finite() {
                    min = min.min(glam::DVec2::new(bounds.min.x, bounds.min.y));
                    max = max.max(glam::DVec2::new(bounds.max.x, bounds.max.y));
                }
            }
        }
        let size = (max - min).max(glam::DVec2::ZERO);
        // The table's own vertical margin where a cell does not override it.
        let table_v_margin = self.tabs[i].scene.table_cells.and_then(|cells| {
            match document.get_entity(cells.table) {
                Some(codec::EntityType::Table(table)) => {
                    let style = table.table_style_handle.and_then(|handle| {
                        match document.objects.get(&handle) {
                            Some(codec::objects::ObjectType::TableStyle(style)) => Some(style),
                            _ => None,
                        }
                    });
                    Some(crate::entities::table::resolved_table_margins(table, style).1)
                }
                _ => None,
            }
        });
        let table_v_margin = table_v_margin.unwrap_or(0.0);
        let scale = crate::entities::common::parse_f64(&state.scale)
            .filter(|v| *v > 0.0)
            .unwrap_or(1.0);
        let rotation = crate::entities::common::parse_f64(&state.rotation)
            .unwrap_or(0.0)
            .to_radians();
        let (sin, cos) = rotation.sin_cos();
        let rotated_height = (size.x * sin).abs() + (size.y * cos).abs();
        let picked = self.picked_cell_positions(i);
        let autofit = state.autofit;
        let alignment = state.alignment;
        self.edit_picked_table(i, "TABLE CELL BLOCK", |table| {
            use codec::entities::table::CellStylePropertyFlags as P;
            let mut changed = false;
            for &(row, column) in &picked {
                let editable = table.cell(row, column).is_some_and(|cell| {
                    !cell
                        .state
                        .intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY)
                });
                if !editable {
                    continue;
                }
                let margins = table.rows.get(row).zip(table.cell(row, column)).map(|(r, cell)| {
                    let margin = |property: P| {
                        crate::entities::table::style_for_property(table, r, column, cell, property)
                            .map(|style| {
                                if property == P::MARGIN_TOP {
                                    style.margin_top
                                } else {
                                    style.margin_bottom
                                }
                            })
                            .unwrap_or(table_v_margin)
                    };
                    margin(P::MARGIN_TOP) + margin(P::MARGIN_BOTTOM)
                });
                let Some(cell) = table.cell_mut(row, column) else {
                    continue;
                };
                let mut content = codec::entities::table::CellContent::block(block);
                content.scale = scale;
                content.rotation = rotation;
                content.format_property_flags |= (P::BLOCK_SCALE | P::ROTATION).bits() as i32;
                cell.contents = vec![content];
                cell.cell_type = codec::entities::table::CellType::Block;
                cell.auto_fit = autofit;
                let style = cell.style.get_or_insert_with(Default::default);
                style.alignment = alignment;
                style.property_flags.insert(P::ALIGNMENT);
                if !autofit {
                    let needed = rotated_height * scale + margins.unwrap_or(0.0);
                    if let Some(r) = table.rows.get_mut(row) {
                        if r.height < needed {
                            r.height = needed;
                        }
                    }
                }
                changed = true;
            }
            changed
        });
    }

    /// The Cell Data Format settings on the picked cells: the data type and
    /// format go into each cell's style, and its value is converted and
    /// shown in the format.
    /// Give the picked cells the data format of `state`. A value that does
    /// not read as the type (a date or point) first asks, in the invalid
    /// data type warning, whether to go on; `confirmed` is its OK.
    fn apply_cell_format(
        &mut self,
        i: usize,
        owned: crate::ui::window::table_cell_dialog::CellFormatState,
        confirmed: bool,
    ) {
        let state = &owned;
        use crate::ui::window::table_cell_dialog::FORMAT_KINDS;
        let picture = state.picture();
        let Some(&(_, data_type, unit_type)) = crate::entities::table::CELL_DATA_CATEGORIES
            .iter()
            .find(|(name, _, _)| *name == FORMAT_KINDS[state.kind.min(8)])
        else {
            return;
        };
        let picked = self.picked_cell_positions(i);
        let Some(cells) = self.tabs[i].scene.table_cells else {
            return;
        };
        let document = &self.tabs[i].scene.document;
        use crate::entities::table::TypedValue;
        let values: Vec<((usize, usize), Option<TypedValue>)> =
            match document.get_entity(cells.table) {
                Some(codec::EntityType::Table(table)) => picked
                    .iter()
                    .map(|&(row, column)| {
                        let value = table
                            .cell(row, column)
                            .and_then(|cell| cell.contents.first())
                            .filter(|content| {
                                content.block_handle.is_none() && content.field_handle.is_none()
                            })
                            .map(|content| {
                                crate::entities::table::typed_cell_value(
                                    document,
                                    &content.value,
                                    state.kind,
                                    &picture,
                                )
                            });
                        ((row, column), value)
                    })
                    .collect(),
                _ => return,
            };
        let invalid: Vec<String> = values
            .iter()
            .filter(|(_, value)| matches!(value, Some(TypedValue::Invalid)))
            .map(|((row, column), _)| format!("{}{}", super::field::column_letters(*column), row + 1))
            .collect();
        if !invalid.is_empty() && !confirmed {
            self.table_cell_warning = Some((owned, invalid));
            self.active_modal = Some(crate::app::ModalKind::TableCellWarning);
            return;
        }
        let kind = state.kind;
        self.edit_picked_table(i, "TABLE CELL FORMAT", |table| {
            use codec::entities::table::CellStylePropertyFlags as P;
            let mut changed = false;
            for ((row, column), value) in values {
                let Some(cell) = table.cell_mut(row, column) else {
                    continue;
                };
                if cell
                    .state
                    .intersects(CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY)
                {
                    continue;
                }
                crate::entities::table::release_content_format(cell, P::DATA_TYPE | P::DATA_FORMAT);
                let style = cell.style.get_or_insert_with(Default::default);
                style.value_data_type = data_type;
                style.value_unit_type = unit_type;
                style.value_format = picture.clone();
                style.property_flags.insert(P::DATA_TYPE | P::DATA_FORMAT);
                match (value, cell.contents.first_mut()) {
                    (Some(TypedValue::Converted(value)), Some(content)) => content.value = value,
                    (Some(TypedValue::Invalid), Some(content)) => {
                        content.value = if kind == 2 {
                            crate::entities::table::empty_date_cell_value()
                        } else {
                            codec::entities::table::CellValue::new()
                        };
                    }
                    _ => {}
                }
                changed = true;
            }
            changed
        });
    }
}
