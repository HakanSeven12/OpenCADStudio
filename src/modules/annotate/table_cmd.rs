// TABLE command — create a styled empty table.
//
// Title/header rows come from the selected table style and are not counted as
// data rows. The command remembers its previous settings and supports either a
// fixed insertion point or a two-corner sizing window.

use codec::entities::table::{CellRange, CellStyle, CellStylePropertyFlags};
use codec::entities::TableBuilder;
use codec::types::Vector3;
use codec::EntityType;
use glam::DVec3;
use std::sync::Mutex;

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

/// Table settings TABLE and -TABLE share and remember: the dialog's column
/// and row settings, its insertion behaviour and its window choices.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TableDefaults {
    pub columns: usize,
    pub data_rows: usize,
    /// Column width; 0 until chosen, then the drawing's default (`cell_width`).
    pub column_width: f64,
    /// Minimum row height, in lines of the row's text.
    pub row_lines: usize,
    pub window: bool,
    /// Window mode keeps the column count (else the column width).
    pub columns_fixed: bool,
    /// Window mode keeps the data row count (else the row height).
    pub rows_fixed: bool,
}

impl Default for TableDefaults {
    fn default() -> Self {
        Self {
            columns: 5,
            data_rows: 1,
            column_width: 0.0,
            row_lines: 1,
            window: false,
            columns_fixed: true,
            rows_fixed: true,
        }
    }
}

impl TableDefaults {
    /// The column width, 2.5 units (63.5 in a metric drawing) until one is set.
    pub fn cell_width(&self, metric: bool) -> f64 {
        if self.column_width > 0.0 {
            self.column_width
        } else if metric {
            63.5
        } else {
            2.5
        }
    }
}

static SESSION: Mutex<TableDefaults> = Mutex::new(TableDefaults {
    columns: 5,
    data_rows: 1,
    column_width: 0.0,
    row_lines: 1,
    window: false,
    columns_fixed: true,
    rows_fixed: true,
});

pub fn session() -> std::sync::MutexGuard<'static, TableDefaults> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// Row heights of a table: a row is `lines` lines of its text — four thirds
/// of the text height each — plus the style's top and bottom margins.
pub fn row_heights(
    style: Option<&codec::objects::TableStyle>,
    title: bool,
    header: bool,
    data_rows: usize,
    lines: usize,
) -> Vec<f64> {
    let (title_h, header_h, data_h, margin) = style.map_or((0.25, 0.18, 0.18, 0.06), |s| {
        (
            s.title_row_style.text_height,
            s.header_row_style.text_height,
            s.data_row_style.text_height,
            s.vertical_margin,
        )
    });
    let height = |text: f64| text * 4.0 / 3.0 * lines.max(1) as f64 + 2.0 * margin;
    let mut rows = Vec::with_capacity(data_rows + 2);
    if title {
        rows.push(height(title_h));
    }
    if header {
        rows.push(height(header_h));
    }
    rows.extend(std::iter::repeat_n(height(data_h), data_rows));
    rows
}

/// The named table styles -TABLE's Style option may pick.
#[derive(Clone)]
pub struct StyleChoice {
    pub name: String,
    pub handle: codec::Handle,
    pub style: codec::objects::TableStyle,
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Columns,
    Rows,
    Insertion,
    FirstCorner,
    SecondCorner(DVec3),
    StyleName { window: bool },
    Width { window: bool },
    Height { window: bool },
    LinkName,
}

pub struct TableCommand {
    step: Step,
    /// "-TABLE" asks for everything on the command line; "TABLE" places the
    /// table the dialog set up.
    command: &'static str,
    style: Option<StyleChoice>,
    styles: Vec<StyleChoice>,
    links: Vec<(String, codec::Handle)>,
    /// None: Auto (window mode fills the window with it).
    columns: Option<usize>,
    data_rows: Option<usize>,
    cell_width: f64,
    row_lines: usize,
    row_style_overrides: Option<[CellStyle; 3]>,
    preview_enabled: bool,
    plane: WorkingPlane,
}

fn cell_style_from_row(style: &codec::objects::RowCellStyle) -> CellStyle {
    let mut result = CellStyle::new();
    result.property_flags = CellStylePropertyFlags::DATA_TYPE
        | CellStylePropertyFlags::DATA_FORMAT
        | CellStylePropertyFlags::ALIGNMENT
        | CellStylePropertyFlags::CONTENT_COLOR
        | CellStylePropertyFlags::TEXT_STYLE
        | CellStylePropertyFlags::TEXT_HEIGHT
        | CellStylePropertyFlags::BACKGROUND_COLOR;
    result.content_color = style.text_color;
    result.text_style_handle = style.text_style_handle;
    result.text_style_name = style.text_style_name.clone();
    result.text_height = style.text_height;
    result.alignment = style.alignment as i32;
    result.background_color = style.fill_color;
    result.fill_enabled = style.fill_enabled;
    result.value_data_type = style.data_type;
    result.value_unit_type = style.unit_type;
    result.value_format = style.format_string.clone();
    result
}

pub(crate) fn selected_row_style(table_style: &codec::objects::TableStyle, name: &str) -> CellStyle {
    let style = if name.eq_ignore_ascii_case("Title") {
        &table_style.title_row_style
    } else if name.eq_ignore_ascii_case("Header") {
        &table_style.header_row_style
    } else {
        &table_style.data_row_style
    };
    cell_style_from_row(style)
}

/// A whole number the column and row prompts accept.
enum Count {
    Value(usize),
    NotPositive,
    Invalid,
}

fn parse_count(input: &str) -> Count {
    match input.parse::<i64>() {
        Ok(value) if value > 0 => Count::Value(value as usize),
        Ok(_) => Count::NotPositive,
        Err(_) => Count::Invalid,
    }
}

/// `input` is a non-empty prefix of the keyword `word`.
fn keyword(input: &str, word: &str) -> bool {
    !input.is_empty() && word.starts_with(&input.to_ascii_uppercase())
}

impl TableCommand {
    fn new(
        command: &'static str,
        style: Option<StyleChoice>,
        defaults: TableDefaults,
        metric: bool,
    ) -> Self {
        Self {
            step: Step::Columns,
            command,
            row_style_overrides: style.as_ref().map(|choice| {
                ["Title", "Header", "Data"].map(|name| selected_row_style(&choice.style, name))
            }),
            style,
            styles: Vec::new(),
            links: Vec::new(),
            columns: Some(defaults.columns.max(1)),
            data_rows: Some(defaults.data_rows.max(1)),
            cell_width: defaults.cell_width(metric),
            row_lines: defaults.row_lines.max(1),
            preview_enabled: true,
            plane: WorkingPlane::default(),
        }
    }

    /// -TABLE: every setting on the command line, starting from the ones
    /// TABLE and -TABLE last used.
    pub fn command_line(
        style: Option<StyleChoice>,
        styles: Vec<StyleChoice>,
        links: Vec<(String, codec::Handle)>,
        metric: bool,
    ) -> Self {
        let defaults = *session();
        let mut command = Self::new("-TABLE", style, defaults, metric);
        command.styles = styles;
        command.links = links;
        command
    }

    /// The line -TABLE starts with.
    pub fn status_line(&self) -> String {
        t!(
            "Current table style: %{style}  Cell width: %{width}  Cell height: %{lines} line(s)",
            style = self.style.as_ref().map_or("Standard", |s| s.name.as_str()),
            width = crate::entities::common::format_length(self.cell_width),
            lines = self.row_lines
        )
        .into_owned()
    }

    /// TABLE: place the table the Insert Table dialog set up. In window
    /// mode a count left out (`None`) comes from the window.
    #[allow(clippy::too_many_arguments)]
    pub fn configured(
        style: Option<StyleChoice>,
        columns: Option<usize>,
        data_rows: Option<usize>,
        column_width: f64,
        row_lines: usize,
        window: bool,
        preview_enabled: bool,
        row_style_names: [&str; 3],
    ) -> Self {
        let mut command = Self::new("TABLE", style, TableDefaults::default(), true);
        command.columns = columns;
        command.data_rows = data_rows;
        command.cell_width = column_width;
        command.row_lines = row_lines.max(1);
        command.preview_enabled = preview_enabled;
        command.row_style_overrides = command.style.as_ref().map(|choice| {
            row_style_names.map(|name| selected_row_style(&choice.style, name))
        });
        command.step = if window {
            Step::FirstCorner
        } else {
            Step::Insertion
        };
        command
    }

    fn dash(&self) -> bool {
        self.command == "-TABLE"
    }

    /// A new table always starts with its title and header rows; the
    /// style's legacy suppress flags do not take them away.
    fn title_header(&self) -> (bool, bool) {
        (true, true)
    }

    /// Column widths, row heights and the insertion point (plane
    /// coordinates) of the table placed at `first`, or in the window from
    /// `first` to `second`. The window's first corner is the insertion
    /// point whichever way the window runs.
    fn layout(&self, first: DVec3, second: Option<DVec3>) -> (DVec3, Vec<f64>, Vec<f64>) {
        let first = self.plane.to_local(first);
        let (title, header) = self.title_header();
        let style = self.style.as_ref().map(|choice| &choice.style);
        let Some(second) = second else {
            let widths = vec![self.cell_width; self.columns.unwrap_or(1)];
            let heights =
                row_heights(style, title, header, self.data_rows.unwrap_or(1), self.row_lines);
            return (first, widths, heights);
        };
        let second = self.plane.to_local(second);
        let (width, height) = ((second.x - first.x).abs(), (second.y - first.y).abs());
        let widths = match self.columns {
            Some(count) if width > 1.0e-9 => vec![width / count as f64; count],
            Some(count) => vec![self.cell_width; count],
            None => {
                let count = ((width / self.cell_width + 1.0e-9).floor() as usize).max(1);
                vec![self.cell_width; count]
            }
        };
        let heights = match self.data_rows {
            // A fixed row count fills the window: one-line rows share what
            // is left of its height.
            Some(count) => {
                let mut rows = row_heights(style, title, header, count, 1);
                let spare = (height - rows.iter().sum::<f64>()) / rows.len() as f64;
                if spare > 0.0 {
                    rows.iter_mut().for_each(|row| *row += spare);
                }
                rows
            }
            // Else as many data rows as fit, at least one.
            None => {
                let one = row_heights(style, title, header, 1, self.row_lines);
                let data = *one.last().unwrap_or(&1.0);
                let labels: f64 = one[..one.len() - 1].iter().sum();
                let count = ((height - labels) / data + 1.0e-9).floor().max(1.0) as usize;
                row_heights(style, title, header, count, self.row_lines)
            }
        };
        (first, widths, heights)
    }

    fn build_table(&self, first: DVec3, second: Option<DVec3>) -> EntityType {
        let (point, widths, heights) = self.layout(first, second);
        let mut table = TableBuilder::new(heights.len(), widths.len())
            .at(Vector3::new(point.x, point.y, point.z))
            .build();
        for (column, width) in table.columns.iter_mut().zip(&widths) {
            column.width = *width;
        }
        for (row, height) in table.rows.iter_mut().zip(&heights) {
            row.height = *height;
        }
        table.table_style_handle = self.style.as_ref().map(|choice| choice.handle);
        if let Some(styles) = &self.row_style_overrides {
            for (row_index, row) in table.rows.iter_mut().enumerate() {
                row.style = Some(styles[row_index.min(2)].clone());
            }
        }
        // The title spans the table, as the title cell style merges its row.
        if self.title_header().0 && widths.len() > 1 {
            table.merge_cells(CellRange::new(0, 0, 0, widths.len() - 1));
        }
        self.plane.place_entity(EntityType::Table(Box::new(table)))
    }

    fn remember(&self) {
        let mut saved = session();
        if let Some(columns) = self.columns {
            saved.columns = columns;
        }
        if let Some(rows) = self.data_rows {
            saved.data_rows = rows;
        }
        saved.column_width = self.cell_width;
        saved.row_lines = self.row_lines;
    }

    fn place(&mut self, first: DVec3, second: Option<DVec3>) -> CmdResult {
        if self.dash() {
            self.remember();
        }
        // Like the dialog's table, the new one opens its first cell.
        CmdResult::CommitAndEditText(self.build_table(first, second))
    }

    fn preview_grid(&self, first: DVec3, second: Option<DVec3>) -> WireModel {
        let (point, widths, heights) = self.layout(first, second);
        let width: f64 = widths.iter().sum();
        let height: f64 = heights.iter().sum();
        let world = |x: f64, y: f64| {
            self.plane
                .to_world(point + DVec3::new(x, y, 0.0))
                .as_vec3()
                .to_array()
        };
        let mut points = Vec::with_capacity((widths.len() + heights.len() + 2) * 2);
        let mut x = 0.0;
        for column in std::iter::once(0.0).chain(widths.iter().copied()) {
            x += column;
            points.push(world(x, 0.0));
            points.push(world(x, -height));
        }
        let mut y = 0.0;
        for row in std::iter::once(0.0).chain(heights.iter().copied()) {
            y -= row;
            points.push(world(0.0, y));
            points.push(world(width, y));
        }
        WireModel {
            name: "table_preview".into(),
            points,
            color: WireModel::CYAN,
            line_weight_px: 1.0,
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            display_visible: true,
            plot_visible: true,
            ..Default::default()
        }
    }

    /// Back to where the Style, Width or Height option was chosen.
    fn back(window: bool) -> Step {
        if window {
            Step::FirstCorner
        } else {
            Step::Insertion
        }
    }

    fn after_counts(&self) -> Step {
        if self.columns.is_some() && self.data_rows.is_some() {
            Step::Insertion
        } else {
            Step::FirstCorner
        }
    }

    /// The options the current placement prompt offers ("" for none).
    fn placement_options(&self) -> &'static str {
        if !self.dash() {
            return "";
        }
        match (self.step, self.columns, self.data_rows) {
            (Step::Insertion, ..) => "Style/Width/Height",
            (_, None, Some(_)) => "Style/Width",
            (_, Some(_), None) => "Style/Height",
            _ => "",
        }
    }
}

impl CadCommand for TableCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        self.command
    }

    fn prompt(&self) -> String {
        let n = self.command;
        match self.step {
            Step::Columns => t!(
                "%{n}  Enter number of columns or [Auto/from Style/data Link] <%{cols}>:",
                n = n,
                cols = self.columns.unwrap_or(1)
            )
            .into_owned(),
            Step::Rows => t!(
                "%{n}  Enter number of rows or [Auto] <%{rows}>:",
                n = n,
                rows = session().data_rows
            )
            .into_owned(),
            Step::Insertion | Step::FirstCorner => {
                let options = self.placement_options();
                match (self.step == Step::Insertion, options.is_empty()) {
                    (true, true) => t!("%{n}  Specify insertion point:", n = n).into_owned(),
                    (true, false) => t!(
                        "%{n}  Specify insertion point or [%{options}]:",
                        n = n,
                        options = options
                    )
                    .into_owned(),
                    (false, true) => t!("%{n}  Specify first corner:", n = n).into_owned(),
                    (false, false) => t!(
                        "%{n}  Specify first corner or [%{options}]:",
                        n = n,
                        options = options
                    )
                    .into_owned(),
                }
            }
            Step::SecondCorner(_) => t!("%{n}  Specify second corner:", n = n).into_owned(),
            Step::StyleName { .. } => t!(
                "%{n}  Enter table style name <%{style}>:",
                n = n,
                style = self.style.as_ref().map_or("Standard", |s| s.name.as_str())
            )
            .into_owned(),
            Step::Width { .. } => t!(
                "%{n}  Enter cell width <%{width}>:",
                n = n,
                width = crate::entities::common::format_length(self.cell_width)
            )
            .into_owned(),
            Step::Height { .. } => t!(
                "%{n}  Enter minimum cell height <%{lines} line>:",
                n = n,
                lines = self.row_lines
            )
            .into_owned(),
            Step::LinkName => {
                t!("%{n}  Enter the name of a data link or [?]:", n = n).into_owned()
            }
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Insertion | Step::FirstCorner | Step::SecondCorner(_) => InputKind::Point,
            Step::StyleName { .. } | Step::LinkName => InputKind::FreeText,
            _ => InputKind::SingleToken,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim();
        let error = |message: &str| Some(CmdResult::ReportError(t!(message).into_owned()));
        match self.step {
            Step::Columns | Step::Rows => {
                let columns = self.step == Step::Columns;
                let value = if input.is_empty() {
                    Some(if columns {
                        self.columns.unwrap_or(1)
                    } else {
                        session().data_rows
                    })
                } else if keyword(input, "AUTO") {
                    None
                } else if columns && keyword(input, "STYLE") {
                    // No table style here carries a starting table.
                    return Some(CmdResult::Measurement(
                        t!("There are no table styles containing tables.  Add a table to a table style by using the TABLESTYLE command.").into_owned(),
                    ));
                } else if columns && keyword(input, "LINK") {
                    if self.links.is_empty() {
                        return Some(CmdResult::Measurement(format!(
                            "{}\n{}",
                            t!("No data links exist in the current drawing."),
                            t!("To create a data link use the DATALINK command.")
                        )));
                    }
                    self.step = Step::LinkName;
                    return Some(CmdResult::NeedPoint);
                } else {
                    match parse_count(input) {
                        Count::Value(value) => Some(value),
                        Count::NotPositive => return error("Value must be positive and nonzero."),
                        Count::Invalid => {
                            return error("Requires a positive nonzero integer or option keyword.")
                        }
                    }
                };
                if columns {
                    self.columns = value;
                    self.step = Step::Rows;
                } else {
                    self.data_rows = value;
                    self.step = self.after_counts();
                }
            }
            Step::Insertion | Step::FirstCorner => {
                let window = self.step == Step::FirstCorner;
                let options = self.placement_options();
                let offered = |word: &str| options.split('/').any(|o| o == word);
                if offered("Style") && keyword(input, "STYLE") {
                    self.step = Step::StyleName { window };
                } else if offered("Width") && keyword(input, "WIDTH") {
                    self.step = Step::Width { window };
                } else if offered("Height") && keyword(input, "HEIGHT") {
                    self.step = Step::Height { window };
                } else if options.is_empty() {
                    return error("Invalid point.");
                } else {
                    return error("Point or option keyword required.");
                }
            }
            Step::SecondCorner(_) => return error("Invalid point."),
            Step::StyleName { window } => {
                self.step = Self::back(window);
                if !input.is_empty() {
                    let Some(choice) = self
                        .styles
                        .iter()
                        .find(|choice| choice.name.eq_ignore_ascii_case(input))
                        .cloned()
                    else {
                        return error("Invalid table style name.");
                    };
                    self.row_style_overrides = Some(
                        ["Title", "Header", "Data"]
                            .map(|name| selected_row_style(&choice.style, name)),
                    );
                    self.style = Some(choice);
                }
            }
            Step::Width { window } => {
                if !input.is_empty() {
                    match input.parse::<f64>() {
                        Ok(value) if value > 0.0 && value.is_finite() => self.cell_width = value,
                        Ok(_) => return error("Value must be positive and nonzero."),
                        Err(_) => return error("Requires numeric distance or two points."),
                    }
                }
                self.step = Self::back(window);
            }
            Step::Height { window } => {
                if !input.is_empty() {
                    match parse_count(input) {
                        Count::Value(value) => self.row_lines = value,
                        Count::NotPositive => return error("Value must be positive and nonzero."),
                        Count::Invalid => return error("Requires a positive nonzero integer."),
                    }
                }
                self.step = Self::back(window);
            }
            Step::LinkName => {
                if input == "?" {
                    let names: Vec<&str> =
                        self.links.iter().map(|(name, _)| name.as_str()).collect();
                    return Some(CmdResult::ReportMeasurement(format!(
                        "{}\n---------------\n{}",
                        t!("Data Link Name"),
                        names.join("\n")
                    )));
                }
                let Some((_, handle)) = self
                    .links
                    .iter()
                    .find(|(name, _)| !input.is_empty() && name.eq_ignore_ascii_case(input))
                else {
                    return error("Data link not found.");
                };
                let style = self
                    .style
                    .as_ref()
                    .map_or_else(|| "Standard".to_string(), |s| s.name.clone());
                return Some(CmdResult::Dispatch(format!(
                    "-TABLE _LINK {:X} {}",
                    handle.value(),
                    style
                )));
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Insertion => self.place(point, None),
            Step::FirstCorner => {
                self.step = Step::SecondCorner(point);
                CmdResult::NeedPoint
            }
            Step::SecondCorner(first) => self.place(first, Some(point)),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        if !self.preview_enabled {
            return None;
        }
        match self.step {
            Step::Insertion => Some(self.preview_grid(point, None)),
            Step::SecondCorner(first) => Some(self.preview_grid(first, Some(point))),
            _ => None,
        }
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["TABLE", "-TABLE"] });

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
    pub fn index(&self, table: &codec::entities::Table) -> usize {
        self.row * table.column_count() + self.column
    }
}

/// Resolve the table cell under `click_world`. Shared by the double-click
/// shortcut and the TABLEDIT command so both agree on grid coordinates,
/// flow direction and locked-cell detection. `table_style` is the style
/// object the table references, when the caller has the document at hand;
/// the table's embedded base style covers the common fallbacks.
pub(crate) fn table_cell_at(
    table: &codec::entities::Table,
    table_style: Option<&codec::objects::TableStyle>,
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
pub(crate) fn cell_locked(table: &codec::entities::Table, row: usize, column: usize) -> bool {
    use codec::entities::table::CellStateFlags;
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
            handle: codec::Handle::NULL,
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
    handle: codec::Handle,
    table: codec::entities::Table,
    row: usize,
    column: usize,
}

impl TableCellEditCommand {
    pub fn new(
        handle: codec::Handle,
        table: &codec::entities::Table,
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
        use codec::entities::table::CellStateFlags;
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
            vec![(
                self.handle,
                vec![EntityType::Table(Box::new(self.table.clone()))],
            )],
            Vec::new(),
        ))
    }

    // Bare Enter (empty buffer) ends the edit *without* writing — the
    // host routes an empty submit here instead of on_text_input(""), so
    // pressing Enter on an untouched cell keeps its current content.
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Done
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
    use codec::entities::table::CellStateFlags;
    use codec::entities::Table;
    use codec::types::Vector3;
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
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(6.0, -0.1, 0.0)),
            None
        );
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(1.0, -9.0, 0.0)),
            None
        );
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
        assert!(
            !table_cell_at(&table, None, DVec3::new(1.25, -0.125, 0.0))
                .expect("neighbour resolves")
                .locked
        );
    }

    #[test]
    fn tabledit_point_hands_point_to_host() {
        let mut cmd = TableditCommand::new();
        assert!(!cmd.needs_entity_pick());
        match cmd.on_point(DVec3::new(1.0, 2.0, 0.0)) {
            CmdResult::EditTableCell { handle, point } => {
                assert_eq!(handle, codec::Handle::NULL);
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
        let handle = codec::Handle::new(42);
        let mut edit_cmd = TableCellEditCommand::new(handle, &table, 0, 0);
        let result = edit_cmd
            .on_text_input("Line1/nLine2\\nLine3\nLine4")
            .expect("cell edit commits");
        match result {
            CmdResult::ReplaceMany(repl, _) => {
                assert_eq!(repl.len(), 1);
                if let codec::EntityType::Table(t) = &repl[0].1[0] {
                    assert_eq!(t.cell_text(0, 0).unwrap(), "Line1\\PLine2\\PLine3\\PLine4");
                } else {
                    panic!("expected Table entity");
                }
            }
            _ => panic!("expected ReplaceMany"),
        }
    }
}
