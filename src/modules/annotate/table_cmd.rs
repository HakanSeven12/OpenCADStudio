// TABLE command — create a styled empty table.
//
// Title/header rows come from the selected table style and are not counted as
// data rows. The command remembers its previous settings and supports either a
// fixed insertion point or a two-corner sizing window.

use acadrust::entities::TableBuilder;
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;
use std::sync::{Mutex, OnceLock};

use crate::command::{CadCommand, CmdResult, InputKind, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/table.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "TABLE",
        label: "Table",
        icon: ICON,
        event: ModuleEvent::Command("TABLE".to_string()),
    }
}

const DEFAULT_COLS: usize = 3;
const DEFAULT_DATA_ROWS: usize = 4;
const DEFAULT_COL_WIDTH: f64 = 2.0;
const DEFAULT_ROW_HEIGHT: f64 = 0.5;

#[derive(Clone, Copy)]
enum InsertionMode {
    Point,
    Window,
}

#[derive(Clone, Copy)]
struct TableDefaults {
    columns: usize,
    data_rows: usize,
    column_width: f64,
    row_height: f64,
    insertion_mode: InsertionMode,
}

impl Default for TableDefaults {
    fn default() -> Self {
        Self {
            columns: DEFAULT_COLS,
            data_rows: DEFAULT_DATA_ROWS,
            column_width: DEFAULT_COL_WIDTH,
            row_height: DEFAULT_ROW_HEIGHT,
            insertion_mode: InsertionMode::Point,
        }
    }
}

fn saved_defaults() -> &'static Mutex<TableDefaults> {
    static DEFAULTS: OnceLock<Mutex<TableDefaults>> = OnceLock::new();
    DEFAULTS.get_or_init(|| Mutex::new(TableDefaults::default()))
}

#[derive(Clone, Copy)]
enum Step {
    Columns,
    DataRows { columns: usize },
    ColumnWidth { columns: usize, data_rows: usize },
    RowHeight {
        columns: usize,
        data_rows: usize,
        column_width: f64,
    },
    InsertionMode {
        columns: usize,
        data_rows: usize,
        column_width: f64,
        row_height: f64,
    },
    Insertion {
        columns: usize,
        data_rows: usize,
        column_width: f64,
        row_height: f64,
    },
    WindowFirst { columns: usize, data_rows: usize },
    WindowSecond {
        columns: usize,
        data_rows: usize,
        first: DVec3,
    },
}

pub struct TableCommand {
    step: Step,
    style_handle: Option<acadrust::Handle>,
    suggested_column_width: f64,
    suggested_row_height: f64,
    preview_scale: f64,
    title_row: bool,
    header_row: bool,
    plane: WorkingPlane,
}

impl TableCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Columns,
            style_handle: None,
            suggested_column_width: DEFAULT_COL_WIDTH,
            suggested_row_height: DEFAULT_ROW_HEIGHT,
            preview_scale: 1.0,
            title_row: true,
            header_row: true,
            plane: WorkingPlane::default(),
        }
    }

    pub fn with_style(
        style_handle: acadrust::Handle,
        style: &acadrust::objects::TableStyle,
        annotation_multiplier: f64,
    ) -> Self {
        let text_height = style
            .data_row_style
            .text_height
            .max(style.header_row_style.text_height)
            .max(style.title_row_style.text_height)
            .max(1.0e-6);
        Self {
            step: Step::Columns,
            style_handle: Some(style_handle),
            suggested_column_width: text_height * 8.0 + style.horizontal_margin * 2.0,
            suggested_row_height: text_height * 1.5 + style.vertical_margin * 2.0,
            preview_scale: if style.annotative {
                annotation_multiplier
            } else {
                1.0
            },
            title_row: !style.title_suppressed,
            header_row: !style.header_suppressed,
            plane: WorkingPlane::default(),
        }
    }

    fn defaults(&self) -> TableDefaults {
        let mut defaults = saved_defaults().lock().map(|value| *value).unwrap_or_default();
        if (defaults.column_width - DEFAULT_COL_WIDTH).abs() < 1.0e-9 {
            defaults.column_width = self.suggested_column_width;
        }
        if (defaults.row_height - DEFAULT_ROW_HEIGHT).abs() < 1.0e-9 {
            defaults.row_height = self.suggested_row_height;
        }
        defaults
    }

    fn total_rows(&self, data_rows: usize) -> usize {
        data_rows + usize::from(self.title_row) + usize::from(self.header_row)
    }

    fn build_table(
        &self,
        point: DVec3,
        rows: usize,
        columns: usize,
        column_width: f64,
        row_height: f64,
    ) -> EntityType {
        let point = self.plane.to_local(point);
        let mut table = TableBuilder::new(rows, columns)
            .at(Vector3::new(point.x, point.y, point.z))
            .row_height(row_height)
            .column_width(column_width)
            .build();
        table.table_style_handle = self.style_handle;
        self.plane.place_entity(EntityType::Table(table))
    }

    fn preview_grid(
        &self,
        point: DVec3,
        rows: usize,
        columns: usize,
        column_width: f64,
        row_height: f64,
        scale: f64,
    ) -> WireModel {
        let point = self.plane.to_local(point);
        let width = columns as f64 * column_width * scale;
        let height = rows as f64 * row_height * scale;
        let mut points = Vec::with_capacity((rows + columns + 2) * 2);
        for column in 0..=columns {
            let x = column as f64 * column_width * scale;
            points.push(self.plane.to_world(point + DVec3::X * x).as_vec3().to_array());
            points.push(
                self.plane
                    .to_world(point + DVec3::new(x, -height, 0.0))
                    .as_vec3()
                    .to_array(),
            );
        }
        for row in 0..=rows {
            let y = -(row as f64 * row_height * scale);
            points.push(self.plane.to_world(point + DVec3::Y * y).as_vec3().to_array());
            points.push(
                self.plane
                    .to_world(point + DVec3::new(width, y, 0.0))
                    .as_vec3()
                    .to_array(),
            );
        }
        WireModel {
            bg_adapt: None,
            point_marker: None,
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: "table_preview".into(),
            points,
            points_low: Vec::new(),
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: Vec::new(),
            tangent_geoms: Vec::new(),
            aci: 0,
            key_vertices: Vec::new(),
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: Vec::new(),
            fill_tris_low: Vec::new(),
        }
    }
}

impl CadCommand for TableCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "TABLE"
    }

    fn prompt(&self) -> String {
        let defaults = self.defaults();
        match &self.step {
            Step::Columns => t!(
                "TABLE  Enter number of columns [%{cols}]:",
                cols = defaults.columns
            )
            .into_owned(),
            Step::DataRows { columns } => t!(
                "TABLE  Enter number of data rows [%{rows}]  (%{cols} cols):",
                rows = defaults.data_rows,
                cols = columns
            )
            .into_owned(),
            Step::ColumnWidth { .. } => t!(
                "TABLE  Enter column width [%{width}]:",
                width = format!("{:.4}", defaults.column_width)
            )
            .into_owned(),
            Step::RowHeight { .. } => t!(
                "TABLE  Enter row height [%{height}]:",
                height = format!("{:.4}", defaults.row_height)
            )
            .into_owned(),
            Step::InsertionMode { .. } => t!(
                "TABLE  Insertion behavior [Point/Window] <%{mode}>:",
                mode = match defaults.insertion_mode {
                    InsertionMode::Point => t!("Point"),
                    InsertionMode::Window => t!("Window"),
                }
            )
            .into_owned(),
            Step::Insertion { columns, data_rows, .. } => t!(
                "TABLE  Specify insertion point  [%{cols}×%{rows}]:",
                cols = columns,
                rows = self.total_rows(*data_rows)
            )
            .into_owned(),
            Step::WindowFirst { .. } => {
                t!("%{n}  Specify first corner:", n = "TABLE").into_owned()
            }
            Step::WindowSecond { .. } => {
                t!("%{n}  Specify opposite corner:", n = "TABLE").into_owned()
            }
        }
    }

    fn input_kind(&self) -> InputKind {
        if matches!(
            self.step,
            Step::Columns
                | Step::DataRows { .. }
                | Step::ColumnWidth { .. }
                | Step::RowHeight { .. }
                | Step::InsertionMode { .. }
        ) {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim();
        let defaults = self.defaults();
        match self.step {
            Step::Columns => {
                let columns = if input.is_empty() {
                    defaults.columns
                } else {
                    input.parse::<usize>().ok().filter(|value| *value > 0)?
                };
                self.step = Step::DataRows { columns };
            }
            Step::DataRows { columns } => {
                let data_rows = if input.is_empty() {
                    defaults.data_rows
                } else {
                    input.parse::<usize>().ok().filter(|value| *value > 0)?
                };
                self.step = Step::ColumnWidth { columns, data_rows };
            }
            Step::ColumnWidth { columns, data_rows } => {
                let column_width = if input.is_empty() {
                    defaults.column_width
                } else {
                    input.parse::<f64>().ok().filter(|value| *value > 0.0)?
                };
                self.step = Step::RowHeight {
                    columns,
                    data_rows,
                    column_width,
                };
            }
            Step::RowHeight { columns, data_rows, column_width } => {
                let row_height = if input.is_empty() {
                    defaults.row_height
                } else {
                    input.parse::<f64>().ok().filter(|value| *value > 0.0)?
                };
                self.step = Step::InsertionMode {
                    columns,
                    data_rows,
                    column_width,
                    row_height,
                };
            }
            Step::InsertionMode { columns, data_rows, column_width, row_height } => {
                let mode = if input.is_empty() {
                    defaults.insertion_mode
                } else if "POINT".starts_with(&input.to_ascii_uppercase()) {
                    InsertionMode::Point
                } else if "WINDOW".starts_with(&input.to_ascii_uppercase()) {
                    InsertionMode::Window
                } else {
                    return None;
                };
                if let Ok(mut saved) = saved_defaults().lock() {
                    *saved = TableDefaults {
                        columns,
                        data_rows,
                        column_width,
                        row_height,
                        insertion_mode: mode,
                    };
                }
                self.step = match mode {
                    InsertionMode::Point => Step::Insertion {
                        columns,
                        data_rows,
                        column_width,
                        row_height,
                    },
                    InsertionMode::Window => Step::WindowFirst { columns, data_rows },
                };
            }
            Step::Insertion { .. } | Step::WindowFirst { .. } | Step::WindowSecond { .. } => {
                return None;
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("")
            .map_or(CmdResult::Cancel, |_| CmdResult::NeedPoint)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Insertion {
                columns,
                data_rows,
                column_width,
                row_height,
            } => CmdResult::CommitAndExit(self.build_table(
                point,
                self.total_rows(data_rows),
                columns,
                column_width,
                row_height,
            )),
            Step::WindowFirst { columns, data_rows } => {
                self.step = Step::WindowSecond {
                    columns,
                    data_rows,
                    first: point,
                };
                CmdResult::NeedPoint
            }
            Step::WindowSecond { columns, data_rows, first } => {
                let first = self.plane.to_local(first);
                let second = self.plane.to_local(point);
                let rows = self.total_rows(data_rows);
                let width = (second.x - first.x).abs().max(1.0e-6);
                let height = (second.y - first.y).abs().max(1.0e-6);
                let top_left = DVec3::new(first.x.min(second.x), first.y.max(second.y), first.z);
                CmdResult::CommitAndExit(self.build_table(
                    self.plane.to_world(top_left),
                    rows,
                    columns,
                    width / columns as f64,
                    height / rows as f64,
                ))
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        match self.step {
            Step::Insertion {
                columns,
                data_rows,
                column_width,
                row_height,
            } => Some(self.preview_grid(
                point,
                self.total_rows(data_rows),
                columns,
                column_width,
                row_height,
                self.preview_scale,
            )),
            Step::WindowSecond { columns, data_rows, first } => {
                let first = self.plane.to_local(first);
                let second = self.plane.to_local(point);
                let rows = self.total_rows(data_rows);
                let width = (second.x - first.x).abs().max(1.0e-6);
                let height = (second.y - first.y).abs().max(1.0e-6);
                let top_left = DVec3::new(first.x.min(second.x), first.y.max(second.y), first.z);
                Some(self.preview_grid(
                    self.plane.to_world(top_left),
                    rows,
                    columns,
                    width / columns as f64,
                    height / rows as f64,
                    1.0,
                ))
            }
            _ => None,
        }
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["TABLE"] });

/// A table cell resolved from a pick: grid coordinates plus whether its
/// content is locked against editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableCellHit {
    pub row: usize,
    pub column: usize,
    pub locked: bool,
}

/// Outcome of launching the table cell editor for a pick (double-click or
/// TABLEDIT). Distinguishes "cell found but locked" — the caller should
/// stop, the indicator is armed — from "nothing to edit" — the caller may
/// fall through to other handlers or re-prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableCellEditStart {
    /// Cell editor launched.
    Started,
    /// Cell resolved but content-locked/read-only: indicator armed, no
    /// editor.
    LockedCell,
    /// Not a table, or the pick missed the grid.
    NoCell,
}

impl TableCellHit {
    /// Flat cell index, matching the Properties palette ordering.
    pub fn index(&self, table: &acadrust::entities::Table) -> usize {
        self.row * table.column_count() + self.column
    }
}

/// Resolve the table cell under `click_world`. Shared by the double-click
/// shortcut and the TABLEDIT command so both agree on grid coordinates,
/// flow direction and locked-cell detection. `table_style` is the style
/// object the table references, when the caller has the document at hand;
/// the table's embedded base style covers the common fallbacks.
pub(crate) fn table_cell_at(
    table: &acadrust::entities::Table,
    table_style: Option<&acadrust::objects::TableStyle>,
    click_world: DVec3,
) -> Option<TableCellHit> {
    let horizontal = glam::DVec3::new(
        table.horizontal_direction.x,
        table.horizontal_direction.y,
        table.horizontal_direction.z,
    )
    .normalize_or(glam::DVec3::X);
    let normal = glam::DVec3::new(table.normal.x, table.normal.y, table.normal.z)
        .normalize_or(glam::DVec3::Z);
    let mut down = horizontal.cross(normal).normalize_or(glam::DVec3::NEG_Y);
    if crate::entities::table::resolved_flow_up(table, table_style) {
        down = -down;
    }
    let origin = glam::DVec3::new(
        table.insertion_point.x,
        table.insertion_point.y,
        table.insertion_point.z,
    );
    let relative = click_world - origin;
    let x = relative.dot(horizontal);
    let y = relative.dot(down);
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let column = table
        .columns
        .iter()
        .scan(0.0, |offset, column| {
            *offset += column.width;
            Some(*offset)
        })
        .position(|end| x <= end)?;
    let row = table
        .rows
        .iter()
        .scan(0.0, |offset, row| {
            *offset += row.height;
            Some(*offset)
        })
        .position(|end| y <= end)?;
    Some(TableCellHit {
        row,
        column,
        locked: cell_locked(table, row, column),
    })
}

/// True when the cell's content may not be edited (locked or read-only).
pub(crate) fn cell_locked(table: &acadrust::entities::Table, row: usize, column: usize) -> bool {
    use acadrust::entities::table::CellStateFlags;
    table.cell(row, column).is_some_and(|cell| {
        cell.state
            .intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY)
    })
}

/// TABLEDIT — edit a table cell's text by picking it. The pick only
/// collects (table, point); the host resolves the cell and launches
/// `TableCellEditCommand`, so the command-first flow and the double-click
/// shortcut share one resolver and one editor setup.
pub struct TableditCommand;

impl TableditCommand {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TableditCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for TableditCommand {
    fn name(&self) -> &'static str {
        "TABLEDIT"
    }

    fn prompt(&self) -> String {
        t!("TABLEDIT  Select a table cell:").into_owned()
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        CmdResult::EditTableCell {
            handle: acadrust::Handle::NULL,
            point,
        }
    }

    // A bare Enter at the pick prompt ends the command (AutoCAD leaves the
    // select-until-valid loop on Enter).
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["TABLEDIT"]
});

pub struct TableCellEditCommand {
    handle: acadrust::Handle,
    table: acadrust::entities::Table,
    row: usize,
    column: usize,
}

impl TableCellEditCommand {
    pub fn new(
        handle: acadrust::Handle,
        table: &acadrust::entities::Table,
        row: usize,
        column: usize,
    ) -> Self {
        Self {
            handle,
            table: table.clone(),
            row,
            column,
        }
    }
}

impl CadCommand for TableCellEditCommand {
    fn name(&self) -> &'static str {
        "TABLE CELL"
    }

    fn prompt(&self) -> String {
        let current = self
            .table
            .cell_text(self.row, self.column)
            .unwrap_or("")
            .replace("\\P", "/n");
        t!(
            "TABLE CELL  Enter text for [%{row},%{column}] <%{current}>:",
            row = self.row,
            column = self.column,
            current = current
        )
        .into_owned()
    }

    // Cell content is free-form prose ("Foo Bar"): Space must type a
    // literal space instead of submitting, Enter finishes, and
    // /n or \n inserts a line break.
    fn input_kind(&self) -> InputKind {
        InputKind::FreeText
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        use acadrust::entities::table::CellStateFlags;
        let cell = self.table.cell_mut(self.row, self.column)?;
        if cell
            .state
            .intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY)
        {
            return Some(CmdResult::Cancel);
        }
        let formatted = text
            .replace("/n", "\\P")
            .replace("\\n", "\\P")
            .replace('\n', "\\P");
        cell.set_text(&formatted);
        self.table.block_record_handle = None;
        Some(CmdResult::ReplaceMany(
            vec![(self.handle, vec![EntityType::Table(self.table.clone())])],
            Vec::new(),
        ))
    }

    // Bare Enter (empty buffer) ends the edit *without* writing — the
    // host routes an empty submit here instead of on_text_input(""), so
    // pressing Enter on an untouched cell keeps its current content.
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tabledit_tests {
    //! `table_cell_at` is the one resolver shared by the double-click
    //! shortcut and the TABLEDIT pick, so its grid math is pinned here.
    //! `Table::new` seeds 2.5-unit columns and 0.25-unit rows with the
    //! default orientation (horizontal +X, flow down).
    use super::{table_cell_at, TableCellEditCommand, TableCellHit, TableditCommand};
    use crate::command::{CadCommand, CmdResult};
    use acadrust::entities::Table;
    use acadrust::entities::table::CellStateFlags;
    use acadrust::types::Vector3;
    use glam::DVec3;

    fn table_2x2() -> Table {
        Table::new(Vector3::new(0.0, 0.0, 0.0), 2, 2)
    }

    #[test]
    fn resolves_row_and_column_from_point() {
        let table = table_2x2();
        // Centers of the four cells.
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(1.25, -0.125, 0.0)),
            Some(TableCellHit {
                row: 0,
                column: 0,
                locked: false
            })
        );
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(3.75, -0.125, 0.0)),
            Some(TableCellHit {
                row: 0,
                column: 1,
                locked: false
            })
        );
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(1.25, -0.375, 0.0)),
            Some(TableCellHit {
                row: 1,
                column: 0,
                locked: false
            })
        );
    }

    #[test]
    fn misses_outside_the_grid() {
        let table = table_2x2();
        // Before the insertion point, beyond the last column/row, and a
        // point in the plane's empty space all miss.
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(-0.1, -0.1, 0.0)),
            None
        );
        assert_eq!(table_cell_at(&table, None, DVec3::new(6.0, -0.1, 0.0)), None);
        assert_eq!(table_cell_at(&table, None, DVec3::new(1.0, -9.0, 0.0)), None);
        assert_eq!(table_cell_at(&table, None, DVec3::new(1.0, 1.0, 0.0)), None);
    }

    #[test]
    fn reports_locked_cells() {
        let mut table = table_2x2();
        if let Some(cell) = table.cell_mut(0, 1) {
            cell.state |= CellStateFlags::CONTENT_LOCKED;
        }
        let hit = table_cell_at(&table, None, DVec3::new(3.75, -0.125, 0.0))
            .expect("locked cell still resolves");
        assert!(hit.locked);
        // Its neighbour stays editable through the same pick path.
        assert!(!table_cell_at(&table, None, DVec3::new(1.25, -0.125, 0.0))
            .expect("neighbour resolves")
            .locked);
    }

    #[test]
    fn tabledit_point_hands_point_to_host() {
        let mut cmd = TableditCommand::new();
        assert!(!cmd.needs_entity_pick());
        match cmd.on_point(DVec3::new(1.0, 2.0, 0.0)) {
            CmdResult::EditTableCell {
                handle,
                point,
            } => {
                assert_eq!(handle, acadrust::Handle::NULL);
                assert_eq!(point, DVec3::new(1.0, 2.0, 0.0));
            }
            _ => panic!("expected EditTableCell"),
        }
        // A bare Enter at the pick prompt ends the command.
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn table_cell_edit_formats_newlines() {
        let table = table_2x2();
        let handle = acadrust::Handle::new(42);
        let mut edit_cmd = TableCellEditCommand::new(handle, &table, 0, 0);
        let result = edit_cmd
            .on_text_input("Line1/nLine2\\nLine3\nLine4")
            .expect("cell edit commits");
        match result {
            CmdResult::ReplaceMany(repl, _) => {
                assert_eq!(repl.len(), 1);
                if let acadrust::EntityType::Table(t) = &repl[0].1[0] {
                    assert_eq!(t.cell_text(0, 0).unwrap(), "Line1\\PLine2\\PLine3\\PLine4");
                } else {
                    panic!("expected Table entity");
                }
            }
            _ => panic!("expected ReplaceMany"),
        }
    }
}
